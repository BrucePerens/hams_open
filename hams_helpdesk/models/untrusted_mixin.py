# Copyright © Bruce Perens K6BP. AGPL-3.0.
"""One choke point for stranger-written text on a ticket.

Every path that creates or edits a hams_helpdesk ticket or posts a ticket message (portal form, mail
ingest, JSON-2 API, GitHub webhook, the simulated-band report, forum, classifieds, DMCA and other
report flows in other modules) ends in create(), write() or message_post() on the ticket. This mixin
overrides all three, so a new path cannot forget the filter. See untrusted_text.py for the filter and
docs/security/TICKET_PROMPT_INJECTION.md for the policy.
"""

import json
import logging

from markupsafe import Markup

from odoo import api, fields, models

from . import untrusted_text as ut

_logger = logging.getLogger(__name__)

# field name -> (max characters, kind). The kind is "html" or "text".
UNTRUSTED_FIELDS = {
    "name": (300, "text"),
    "description": (50000, "html"),
    "callsign": (40, "text"),
    "ncmec_contact_email": (254, "text"),
    "ncmec_contact_phone": (40, "text"),
}
MESSAGE_BODY_LIMIT = 50000
SUSPICION_FIELDS = ("suspicion_score", "suspicious", "suspicion_removed", "suspicion_log_ids")
RAW_EVIDENCE_LIMIT = 100000


class HamsHelpdeskUntrustedMixin(models.AbstractModel):
    _name = "hams_helpdesk.untrusted.mixin"
    _description = "Untrusted text filter for tickets"

    # Other modules extend this to filter their own free-text fields too: override and add to the dict.
    @api.model
    def _untrusted_field_specs(self):
        return dict(UNTRUSTED_FIELDS)

    @api.model
    def _untrusted_filter_vals(self, vals):
        """(new vals, [(field, Result)], {field: raw original}) with every untrusted field filtered."""
        vals = dict(vals)
        for key in SUSPICION_FIELDS:
            vals.pop(key, None)  # nothing a caller sends may set these
        results = []
        raw = {}
        specs = self._untrusted_field_specs()
        for fname, (limit, kind) in specs.items():
            value = vals.get(fname)
            if not value or not isinstance(value, str):
                continue
            result = ut.sanitize_html(value, limit, fname) if kind == "html" else ut.scan_text(value, limit, fname)
            if kind == "html":
                new_value = result.text
            else:
                new_value = result.text.replace("\n", " ") if fname in ("name", "callsign") else result.text
            if new_value != value:
                raw[fname] = str(value)[:RAW_EVIDENCE_LIMIT]
            vals[fname] = Markup(new_value) if kind == "html" else new_value
            results.append((fname, result))
        return vals, results, raw

    @api.model
    def _untrusted_summary(self, results, extra_parts=()):
        findings, removed, score = ut.merge([r for _f, r in results])
        findings.extend(ut.scan_concatenation([r.plain for _f, r in results] + list(extra_parts)))
        score += sum(f["weight"] for f in findings if f["vector"].startswith("split_"))
        return findings, removed, min(score, 100)

    def _untrusted_record(self, findings, removed, score, raw, source):
        """Persist what was found, if anything, on the tickets in self. Uses the helpdesk service
        account: staff can read the log, a portal customer cannot, and no sudo() is involved."""
        if not self or not (findings or removed or raw):
            return
        utils = self.env["zero_sudo.security.utils"]
        hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")
        Log = hd_env["hams_helpdesk.ticket.suspicion"]
        for ticket in self.with_env(hd_env):
            Log.create(
                {
                    "ticket_id": ticket.id,
                    "source": source,
                    "score": score,
                    "removed": removed,
                    "findings_json": ut.findings_to_json(findings),
                    "raw_original": json.dumps(raw, ensure_ascii=True)[: 2 * RAW_EVIDENCE_LIMIT] if raw else False,
                }
            )
            ticket_vals = {
                "suspicion_removed": ticket.suspicion_removed + removed,
                "suspicion_score": max(ticket.suspicion_score, score),
            }
            ticket_vals["suspicious"] = ticket_vals["suspicion_score"] >= ut.SUSPICION_THRESHOLD
            ticket.with_context(untrusted_filter_bypass=True).write(ticket_vals)

    # ---- the three entry points ----

    @api.model_create_multi
    def create(self, vals_list):
        filtered = []
        meta = []
        for vals in vals_list:
            new_vals, results, raw = self._untrusted_filter_vals(vals)
            findings, removed, score = self._untrusted_summary(results)
            if score >= ut.SUSPICION_THRESHOLD:
                new_vals["suspicious"] = True
            new_vals["suspicion_score"] = score
            new_vals["suspicion_removed"] = removed
            filtered.append(new_vals)
            meta.append((findings, removed, score, raw))
        records = super().create(filtered)
        for record, (findings, removed, score, raw) in zip(records, meta):
            record._untrusted_log_only(findings, removed, score, raw, "create")
        return records

    def _untrusted_log_only(self, findings, removed, score, raw, source):
        """Like _untrusted_record but the counters were already stored by create()."""
        if not (findings or removed or raw):
            return
        utils = self.env["zero_sudo.security.utils"]
        hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")
        hd_env["hams_helpdesk.ticket.suspicion"].create(
            {
                "ticket_id": self.id,
                "source": source,
                "score": score,
                "removed": removed,
                "findings_json": ut.findings_to_json(findings),
                "raw_original": json.dumps(raw, ensure_ascii=True)[: 2 * RAW_EVIDENCE_LIMIT] if raw else False,
            }
        )

    def write(self, vals):
        if self.env.context.get("untrusted_filter_bypass"):
            return super().write(vals)
        specs = self._untrusted_field_specs()
        if not (set(vals) & set(specs)) and not (set(vals) & set(SUSPICION_FIELDS)):
            return super().write(vals)
        new_vals, results, raw = self._untrusted_filter_vals(vals)
        result = super().write(new_vals)
        if results:
            findings, removed, score = self._untrusted_summary(results)
            for ticket in self:
                ticket._untrusted_record(findings, removed, score, raw, "write")
        return result

    def message_post(self, **kwargs):
        # Not "if body": a message with only a subject is still text from outside.
        body = kwargs.get("body")
        results = []
        raw = {}
        if body and not self.env.context.get("untrusted_filter_bypass"):
            text = str(body)
            result = ut.sanitize_any(text, MESSAGE_BODY_LIMIT, "message_body")
            if result.text != text:
                raw["message_body"] = text[:RAW_EVIDENCE_LIMIT]
            kwargs["body"] = Markup(result.text) if ut.looks_like_html(text) else result.text
            results.append(("message_body", result))
        for key, limit in (("subject", 300), ("email_from", 300)):
            if isinstance(kwargs.get(key), str) and kwargs[key]:
                head = ut.safe_header(kwargs[key], limit, key)
                if head.text != kwargs[key]:
                    raw[key] = kwargs[key][:1000]
                kwargs[key] = head.text
                results.append((key, head))
        message = super().message_post(**kwargs)
        if results and self:
            findings, removed, score = self._untrusted_summary(results)
            for ticket in self:
                ticket._untrusted_record(findings, removed, score, raw, "message")
        return message

    # ---- the safe view every AI reader uses ----

    def safe_view(self, max_chars=0):
        """One dict per ticket with only filtered, visible text, the findings, and an untrusted
        wrapper. No AI reads a raw ticket body, raw HTML or raw metadata: use this."""
        views = []
        for ticket in self:
            subject = ut.scan_text(ticket.name or "", 300, "name")
            body = ut.sanitize_any(ticket.description or "", 20000, "description")
            call = ut.scan_text(ticket.callsign or "", 40, "callsign")
            results = [subject, body, call]
            findings, removed, score = ut.merge(results)
            findings.extend(ut.scan_concatenation([subject.plain, body.plain]))
            suspicious = bool(ticket.suspicious) or score >= ut.SUSPICION_THRESHOLD
            text = body.plain
            if max_chars and len(text) > max_chars:
                text = "%s... [truncated, %d chars total; read the ticket for the rest]" % (text[:max_chars], len(body.plain))
            payload = "Subject: %s\nCallsign: %s\n\n%s" % (subject.text, call.text, text)
            views.append(
                {
                    "id": ticket.id,
                    "subject": subject.text,
                    "callsign": call.text,
                    "description": body.plain,
                    "untrusted_block": ut.wrap_untrusted(payload, "ticket"),
                    "suspicious": suspicious,
                    "suspicion_score": max(score, ticket.suspicion_score or 0),
                    "removed_or_flagged": removed + len(findings),
                    "findings": [
                        {k: f[k] for k in ("vector", "location", "count", "excerpt")} for f in findings
                    ],
                }
            )
        return views


class HamsHelpdeskTicketSuspicion(models.Model):
    _name = "hams_helpdesk.ticket.suspicion"
    _description = "What the untrusted-text filter removed or flagged on a ticket (staff evidence)"
    _order = "id desc"

    ticket_id = fields.Many2one("hams_helpdesk.ticket", required=True, ondelete="cascade", index=True)
    source = fields.Char()
    score = fields.Integer()
    removed = fields.Integer()
    findings_json = fields.Text(help="List of {vector, location, count, excerpt, weight}; excerpts are escaped.")
    raw_original = fields.Text(help="The text exactly as received, kept for evidence. Staff only. Never given to an AI.")
    findings_summary = fields.Char(compute="_compute_findings_summary")

    @api.depends("findings_json")
    def _compute_findings_summary(self):
        for rec in self:
            items = ut.findings_from_json(rec.findings_json)
            rec.findings_summary = "; ".join(
                "%s x%s in %s" % (f.get("vector"), f.get("count"), f.get("location")) for f in items
            )[:500]
