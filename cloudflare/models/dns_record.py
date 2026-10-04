# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import ipaddress
import re

from odoo import api, fields, models, _
from odoo.exceptions import AccessError, UserError, ValidationError

from ..utils import dns_plan
from ..utils.cloudflare_api import (
    create_dns_record,
    delete_dns_record,
    find_zone_id,
    is_cloudflare_id,
    list_dns_records_named,
    update_dns_record,
)

# A DNS name as Odoo stores it: lower case, no trailing dot, at least two labels; "_" for service and
# verification labels, a leading "*" label for a wildcard.
NAME_PATTERN = re.compile(r"(\*\.)?([a-z0-9_]([a-z0-9_-]{0,61}[a-z0-9_])?\.)+[a-z0-9_]([a-z0-9_-]{0,61}[a-z0-9_])?")
CREDENTIAL_ERROR = "***ERROR***"


class CloudflareDNSRecord(models.Model):
    """One DNS record that Odoo is the source of truth for.

    A push (`dns_push_plan` then `dns_push_apply`) creates and updates only what Odoo has a row for.
    It never overwrites a record it did not create (adopting an existing record only stores its id) and
    never touches a record Odoo has no row for. Removing or archiving a row removes nothing at Cloudflare.
    The one delete is the explicit "retire" flag on a row linked to its record by id.
    """

    _name = "cloudflare.dns.record"
    _description = "Cloudflare DNS Record"

    name = fields.Char(string="Name", required=True, help="The full name, lower case, no trailing dot (callbook.hams.com).")

    _check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )

    type = fields.Selection(
        [("A", "A"), ("AAAA", "AAAA"), ("CNAME", "CNAME"), ("TXT", "TXT"), ("NS", "NS")],
        string="Type",
        required=True,
    )
    content = fields.Char(string="Content", required=True)
    proxied = fields.Boolean(
        string="Proxied",
        default=True,
        help="Only A, AAAA and CNAME records can be proxied. A nameserver name (glue) and every NS and TXT record are DNS-only.",
    )
    website_id = fields.Many2one(
        "website",
        string="Website",
        help="The website whose Cloudflare credentials push this row. Empty: the first website that has an API token.",
    )
    manage = fields.Boolean(
        string="Odoo manages it",
        default=True,
        help="Off: observe only. A push links the row to the matching record and reports differences, and writes nothing at Cloudflare.",
    )
    cf_record_id = fields.Char(
        string="Cloudflare record id",
        copy=False,
        index=True,
        help="Set by a push when it creates or adopts the record. To take over a record that already exists with other "
        "content, paste its id here: the next plan then shows the update.",
    )
    retire = fields.Boolean(
        string="Retire",
        copy=False,
        help="Delete this row's own record at Cloudflare on the next push, if the row is managed and linked to it by id. "
        "The plan shows the delete first; afterwards the row is archived. Nothing else is ever deleted.",
    )
    active = fields.Boolean(default=True)
    cf_last_result = fields.Char(string="Last push result", readonly=True, copy=False)

    @api.onchange("type")
    def _onchange_type_clears_proxied(self):
        """The form's Proxied box defaults on; an NS or TXT row can never be proxied, so choosing one of those
        types clears it instead of leaving the administrator with a row the constraint refuses to save."""
        for rec in self:
            if rec.type not in dns_plan.PROXIABLE_TYPES:
                rec.proxied = False

    # [@ANCHOR: cloudflare:COMM_dns_record_constraints]
    # Verified by [@ANCHOR: COMM_test_dns_record_constraints]
    @api.constrains("name", "type", "content", "proxied", "cf_record_id")
    def _check_record(self):
        same_keys = self.env["cloudflare.dns.record"].search([("name", "in", self.mapped("name"))], limit=10000)
        for rec in self:
            if not NAME_PATTERN.fullmatch(rec.name or ""):
                raise ValidationError(_(
                    "%(name)r is not a DNS name Odoo can push: use the full name in lower case, with no trailing dot "
                    "and at least two labels (callbook.hams.com).", name=rec.name))
            if rec.type == "A":
                self._require_ip(rec, ipaddress.IPv4Address)
            elif rec.type == "AAAA":
                self._require_ip(rec, ipaddress.IPv6Address)
            elif rec.type in ("NS", "CNAME"):
                if not NAME_PATTERN.fullmatch(rec.content or ""):
                    raise ValidationError(_(
                        "%(type)s content %(content)r must be a host name in lower case with no trailing dot.",
                        type=rec.type, content=rec.content))
            elif not (rec.content or "").strip() or len(rec.content) > 2048:
                raise ValidationError(_("TXT content must be 1 to 2048 characters."))
            if rec.proxied and rec.type not in dns_plan.PROXIABLE_TYPES:
                raise ValidationError(_("%(type)s records cannot be proxied: Cloudflare answers them directly.", type=rec.type))
            if rec.cf_record_id and not is_cloudflare_id(rec.cf_record_id):
                raise ValidationError(_("A Cloudflare record id is 32 lower-case hexadecimal characters."))
            clash = same_keys.filtered(
                lambda other: other.id != rec.id and (other.type, other.name, other.content) == (rec.type, rec.name, rec.content))
            if clash:
                raise ValidationError(_("%(type)s %(name)s -> %(content)s already exists.",
                                        type=rec.type, name=rec.name, content=rec.content))
            self._check_glue(rec)

    @staticmethod
    def _require_ip(rec, factory):
        try:
            factory(rec.content or "")
        except ValueError:
            raise ValidationError(_("%(type)s content %(content)r is not an address of that kind.",
                                    type=rec.type, content=rec.content)) from None

    # [@ANCHOR: cloudflare:COMM_dns_record_glue_unproxied]
    @api.model
    def _check_glue(self, rec):
        """A name that an NS record points at must answer with its real address: a proxied A or AAAA
        row there would hand resolvers Cloudflare's addresses instead of the nameserver's."""
        if rec.type in ("A", "AAAA") and rec.proxied:
            if self.env["cloudflare.dns.record"].search_count([("type", "=", "NS"), ("content", "=", rec.name)]):
                raise ValidationError(_("%(name)s is the target of an NS record, so it must be DNS-only (not proxied).",
                                        name=rec.name))
        if rec.type == "NS":
            if self.env["cloudflare.dns.record"].search_count([("type", "in", ("A", "AAAA")), ("name", "=", rec.content), ("proxied", "=", True)]):
                raise ValidationError(_("%(name)s has a proxied address row; a nameserver name must be DNS-only.",
                                        name=rec.content))

    # ------------------------------------------------------------------------------------------
    # Push
    # ------------------------------------------------------------------------------------------
    # [@ANCHOR: cloudflare:COMM_dns_check_caller_authorized]
    def _check_dns_caller_authorized(self):
        if not (self.env.is_superuser() or self.env.user.has_group("base.group_system")):
            raise AccessError(_("Only an administrator may push DNS records to Cloudflare."))

    def _dns_credential_website(self, row, websites):
        """The website whose token pushes `row`: its own website, else the first of `websites` (every website,
        by id, fetched once by the caller) that has a token. Returns (website, token, problem)."""
        if row.website_id:
            token, _zone = row.website_id._get_cloudflare_credentials()
            if not token or token == CREDENTIAL_ERROR:
                return row.website_id, None, f"website {row.website_id.display_name} has no usable Cloudflare API token"
            return row.website_id, token, None
        for website in websites:
            token, _zone = website._get_cloudflare_credentials()
            if token and token != CREDENTIAL_ERROR:
                return website, token, None
        return None, None, "no website has a Cloudflare API token"

    # [@ANCHOR: cloudflare:COMM_dns_push_plan]
    def dns_push_plan(self):
        """What a push of these rows would do, computed from Cloudflare's current state with reads only.

        Returns {"entries": [...], "problems": [...], "hash": str, "text": str}. Nothing is written
        anywhere. A row that cannot be planned (no token, no zone, Cloudflare unreachable) is reported as a
        `problem` entry and does not hold back the other rows.
        """
        self._check_dns_caller_authorized()
        zone_cache, record_cache = {}, {}
        entries = []
        websites = self.env["website"].search([], order="id", limit=1000)
        for row in self.sorted("id"):
            plain = {"id": row.id, "name": row.name, "type": row.type, "content": row.content,
                     "proxied": row.proxied, "manage": row.manage, "cf_record_id": row.cf_record_id or "",
                     "retire": row.retire}
            website, token, problem = self._dns_credential_website(row, websites)
            if problem:
                entries.append(self._dns_problem_entry(plain, problem))
                continue
            zone_id, problem = self._dns_zone_of(row.name, website, token, zone_cache)
            if problem:
                entries.append(self._dns_problem_entry(plain, problem))
                continue
            key = (zone_id, row.name)
            if key not in record_cache:
                record_cache[key] = list_dns_records_named(zone_id, row.name, token)
            ok, remote = record_cache[key]
            if not ok:
                entries.append(self._dns_problem_entry(plain, f"Cloudflare could not be read ({remote}); nothing is planned for this row"))
                continue
            entry = dns_plan.plan_row(plain, remote)
            entry["zone_id"] = zone_id
            entry["website_id"] = website.id
            entries.append(entry)
        problems = self._dns_problems(entries)
        return {"entries": entries, "problems": problems, "hash": dns_plan.plan_hash(entries),
                "text": dns_plan.render(entries)}

    @staticmethod
    def _dns_problem_entry(plain, reason):
        return {"action": dns_plan.PROBLEM, "row_id": plain["id"], "type": plain["type"], "name": plain["name"],
                "content": plain["content"], "reason": reason,
                "proxied": dns_plan.effective_proxied(plain["type"], plain["proxied"])}

    @api.model
    def _dns_zone_of(self, name, website, token, cache):
        """The zone that holds `name`: the longest suffix of it, of two labels or more, that is a zone the
        token can see. Returns (zone_id, problem)."""
        labels = name.split(".")
        for start in range(0, len(labels) - 1):
            candidate = ".".join(labels[start:])
            key = (website.id, candidate)
            if key not in cache:
                cache[key] = find_zone_id(candidate, token)
            ok, zone_id = cache[key]
            if not ok:
                return None, "Cloudflare could not be asked for the zone (API error); nothing is planned for this row"
            if zone_id:
                return zone_id, None
        return None, f"no Cloudflare zone for {name} is visible to the token of website {website.display_name}"

    # [@ANCHOR: cloudflare:COMM_dns_problems_hook]
    def _dns_problems(self, entries):
        """Reasons the whole push must be refused (strings; empty = fine). A hook: other modules extend it
        the way `_ingress_problems` is extended for tunnel pushes."""
        return []

    # [@ANCHOR: cloudflare:COMM_dns_push_apply]
    def dns_push_apply(self, expected_hash):
        """Apply exactly the plan whose hash the administrator saw. The plan is recomputed first; if Cloudflare
        or Odoo changed since, nothing is applied. Returns a list of per-row result strings.

        create: POST a new record. update: PUT the linked record. adopt: store the Cloudflare id in the row,
        no write at Cloudflare. delete: only for a row marked retire and linked by id; the row is archived.
        """
        self._check_dns_caller_authorized()
        plan = self.dns_push_plan()
        if plan["problems"]:
            raise UserError(_("The DNS push was refused: %s", "; ".join(plan["problems"])))
        if not expected_hash or plan["hash"] != expected_hash:
            raise UserError(_("The records or Cloudflare changed since the plan was made. Nothing was applied: plan again."))
        results = []
        tokens = {}
        Record = self.env["cloudflare.dns.record"]
        for entry in plan["entries"]:
            row = Record.browse(entry["row_id"])
            action = entry["action"]
            if action == dns_plan.ADOPT:
                row.write({"cf_record_id": entry["remote_id"], "cf_last_result": "adopted (not changed at Cloudflare)"})
                results.append(f"adopted {entry['type']} {entry['name']}")
            elif entry.get("already_gone"):
                row.write({"active": False, "retire": False, "cf_record_id": False,
                           "cf_last_result": "already absent at Cloudflare"})
                results.append(f"archived {entry['type']} {entry['name']} (already absent at Cloudflare)")
            elif action in dns_plan.WRITING_ACTIONS:
                website = self.env["website"].browse(entry["website_id"])
                token = tokens.get(website.id)
                if token is None:
                    token = tokens[website.id] = website._get_cloudflare_credentials()[0]
                payload = {"type": entry["type"], "name": entry["name"], "content": row.content,
                           "proxied": entry["proxied"], "ttl": entry.get("ttl", 1)}
                if action == dns_plan.DELETE:
                    ok, value = delete_dns_record(entry["zone_id"], entry["remote_id"], token)
                    if ok:
                        row.write({"active": False, "retire": False, "cf_record_id": False,
                                   "cf_last_result": "deleted at Cloudflare"})
                        results.append(f"deleted {entry['type']} {entry['name']}")
                    else:
                        row.write({"cf_last_result": f"delete failed: {value}"})
                        results.append(f"FAILED delete {entry['type']} {entry['name']}: {value}")
                    continue
                if action == dns_plan.CREATE:
                    ok, value = create_dns_record(entry["zone_id"], payload, token)
                else:
                    ok, value = update_dns_record(entry["zone_id"], entry["remote_id"], payload, token)
                if ok:
                    vals = {"cf_last_result": f"{action}d"}
                    if action == dns_plan.CREATE and is_cloudflare_id(value):
                        vals["cf_record_id"] = value
                    row.write(vals)
                    results.append(f"{action}d {entry['type']} {entry['name']}")
                else:
                    row.write({"cf_last_result": f"{action} failed: {value}"})
                    results.append(f"FAILED {action} {entry['type']} {entry['name']}: {value}")
        return results
