# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The signed-in members an automatic exception ticket affected, so the site team can write to them.

One ``hams_helpdesk.exception.member`` row per (exception fingerprint, member). It LINKS to the member's
``res.users`` record and stores nothing about them but the link, when they were first and last affected
and how many reports they were part of. The name, callsign(s) and email shown on the ticket are computed
live from the member's own records every time the ticket is read, so anything the member changes or has
erased (the account is deactivated and its login rewritten by the erasure chain) is gone from every old
ticket with no cleanup of the tickets. Nothing here is ever copied into the ticket text, a sample, a
log line, a fingerprint or any message (see ``exception_fingerprint._ticket_body``).

Only helpdesk staff can read these rows or the computed details (``ir.model.access.csv`` and the
``groups`` on the ticket's fields); the reporter service account writes them.

Minors: ``_contact_user`` is the extension point. ``hams_helpdesk`` cannot know who is a minor under a
guardian's supervision (that model lives in ham_communications_consent in hams_com), so the base
implementation always answers "the member"; ham_communications_consent overrides it to answer the
supervising guardian, or nobody.
"""

from urllib.parse import quote

from odoo import api, fields, models
from odoo.exceptions import UserError

# Most members listed per fingerprint. A failure that hits more distinct members than this is a site-wide
# fault, not a handful of people to write to; the rest are only counted (fingerprint.unlisted_member_count).
MEMBER_LIST_LIMIT = 25
# Sources with a signed-in member behind them. A cron job, a mail the server failed to send and an external
# daemon have no member; the user those carry is the job's own account, which is not an affected person.
MEMBER_SOURCES = ("http", "rpc", "browser")
STAFF_GROUPS = "hams_helpdesk.group_helpdesk_user,base.group_system"

KIND_MEMBER = "member"
KIND_GUARDIAN = "guardian"
KIND_WITHHELD = "withheld"
KIND_CLOSED = "closed"

CALLBOOK_SERVICE_XMLID = "ham_callbook.user_callbook_api_service"

REPLY_SUBJECT = "hams.com: we are working on a problem you ran into"
REPLY_BODY = (
    "<p>Hello,</p>"
    "<p>The hams.com site team saw an error while the account was using the site, and we are working on it. "
    "You do not need to do anything. We will write again if we need to ask you something.</p>"
    "<p>Thank you for your patience.<br/>The hams.com site team</p>"
)


class ExceptionMember(models.Model):
    _name = "hams_helpdesk.exception.member"
    _description = "Member affected by an automatic exception ticket"
    _order = "last_seen desc, id desc"

    name = fields.Char(compute="_compute_name", help="A number only; the member's name is never part of it.")
    fingerprint_id = fields.Many2one(
        "hams_helpdesk.exception.fingerprint", required=True, index=True, ondelete="cascade", readonly=True
    )
    ticket_id = fields.Many2one(
        "hams_helpdesk.ticket", related="fingerprint_id.ticket_id", store=True, index=True, readonly=True
    )
    user_id = fields.Many2one(
        "res.users",
        string="Member account",
        index=True,
        ondelete="set null",
        readonly=True,
        help="A link to the account, never a copy of the name, callsign or email.",
    )
    first_seen = fields.Datetime(readonly=True)
    last_seen = fields.Datetime(readonly=True)
    occurrence_count = fields.Integer(
        string="Reports",
        readonly=True,
        help="Reports this member was part of. A lower bound: repeats held back by the once-a-minute gate "
        "cannot be told apart by member and are only counted on the ticket.",
    )
    # Computed live, never stored.
    member_name = fields.Char(string="Member", compute="_compute_details", groups=STAFF_GROUPS)
    callsigns = fields.Char(string="Callsign(s)", compute="_compute_details", groups=STAFF_GROUPS)
    contact_kind = fields.Selection(
        [
            (KIND_MEMBER, "The member"),
            (KIND_GUARDIAN, "Guardian of a minor member"),
            (KIND_WITHHELD, "Minor member, no guardian contact"),
            (KIND_CLOSED, "Account closed"),
        ],
        compute="_compute_details",
        groups=STAFF_GROUPS,
    )
    contact_label = fields.Char(string="Write to", compute="_compute_details", groups=STAFF_GROUPS)
    contact_name = fields.Char(string="Contact name", compute="_compute_details", groups=STAFF_GROUPS)
    contact_email = fields.Char(string="Email", compute="_compute_details", groups=STAFF_GROUPS)

    _member_once = models.Constraint(
        "UNIQUE(fingerprint_id, user_id)", "A member is listed once per exception fingerprint."
    )

    # [@ANCHOR: hams_helpdesk:exc_reportable_member]
    @api.model
    def _reportable_member_uid(self, uid):
        """``uid`` when it is a real, active person's account, else None: not the public (logged-out) user, not
        OdooBot, not a service account, not an archived account. Read with SQL on the writer's own cursor
        because ``is_service_account`` is restricted to administrators and the request's transaction may be
        aborted."""
        if not isinstance(uid, int) or isinstance(uid, bool) or uid <= 0:
            return None
        self.env.cr.execute(
            "SELECT u.id FROM res_users u WHERE u.id = %s AND u.active AND NOT COALESCE(u.is_service_account, false) "
            "AND NOT EXISTS (SELECT 1 FROM ir_model_data d WHERE d.model = 'res.users' AND d.res_id = u.id "
            "AND d.module = 'base' AND d.name IN ('public_user', 'user_root'))",
            (uid,),
        )  # audit-ignore-sql: parameterised, read-only
        row = self.env.cr.fetchone()
        return row[0] if row else None

    # [@ANCHOR: hams_helpdesk:exc_note_member]
    @api.model
    def _note_member(self, fingerprint, uid, now):
        """Records that ``uid`` was part of one admitted report of ``fingerprint``. Returns the member's res.users
        id when it was linked, else None (not a person, or the list is full and the member only counted)."""
        member_uid = self._reportable_member_uid(uid)
        if not member_uid:
            return None
        row = self.search([("fingerprint_id", "=", fingerprint.id), ("user_id", "=", member_uid)], limit=1)
        if row:
            row.write({"last_seen": now, "occurrence_count": row.occurrence_count + 1})
        elif self.search_count([("fingerprint_id", "=", fingerprint.id)]) < MEMBER_LIST_LIMIT:
            self.create(
                {
                    "fingerprint_id": fingerprint.id,
                    "user_id": member_uid,
                    "first_seen": now,
                    "last_seen": now,
                    "occurrence_count": 1,
                }
            )
        else:
            fingerprint.unlisted_member_count += 1
            return None
        fingerprint.last_member_id = member_uid
        return member_uid

    @api.model
    def _is_listed(self, fingerprint, uid):
        """True when ``uid`` already has a row for ``fingerprint``, or the list is full (so a new member would
        not be listed anyway)."""
        if self.search_count([("fingerprint_id", "=", fingerprint.id), ("user_id", "=", uid)]):
            return True
        return self.search_count([("fingerprint_id", "=", fingerprint.id)]) >= MEMBER_LIST_LIMIT

    # [@ANCHOR: hams_helpdesk:exc_contact_user]
    def _contact_user(self, user):
        """(kind, the res.users to write to) for the member ``user``. The extension point for a deployment
        that knows some members are minors: it answers the guardian (KIND_GUARDIAN), or an empty recordset with
        KIND_WITHHELD when the minor's guardian cannot be found. Here every active member is written to
        directly. Runs as the staff member reading the ticket."""
        return KIND_MEMBER, user

    @api.model
    def _callsigns_of(self, users):
        """{user id: [callsigns]} from the account's own callsign and the callbook rows linked to the account.
        The callbook is read through its read-only service account: helpdesk staff have no access to it."""
        found = {user.id: [] for user in users}
        for user in users:
            if user.callsign:
                found[user.id].append(user.callsign.strip().upper())
        service = self.env["zero_sudo.security.utils"]._get_service_env(CALLBOOK_SERVICE_XMLID)
        rows = service["ham.callbook"].search([("user_id", "in", users.ids)], limit=20 * max(1, len(users)))
        for row in rows:
            call = (row.callsign or "").strip().upper()
            if call and call not in found[row.user_id.id]:
                found[row.user_id.id].append(call)
        return found

    @api.depends("fingerprint_id")
    def _compute_name(self):
        for row in self:
            row.name = self.env._("Affected member %(number)s", number=row.id or "")

    @api.depends("user_id")
    def _compute_details(self):
        users = self.mapped("user_id")
        # A closed (archived or erased) account shows nothing about the person.
        live = users.filtered("active")
        calls = self._callsigns_of(live) if live else {}
        for row in self:
            user = row.user_id
            row.member_name = row.callsigns = row.contact_name = row.contact_email = row.contact_label = False
            row.contact_kind = KIND_CLOSED
            if not user:
                row.contact_label = self.env._("The account no longer exists")
                continue
            if not user.active:
                row.contact_label = self.env._("The account is closed; nothing about the member is kept here")
                continue
            row.member_name = user.name
            row.callsigns = ", ".join(calls.get(user.id, [])) or False
            kind, contact = row._contact_user(user)
            row.contact_kind = kind
            if kind == KIND_GUARDIAN:
                row.contact_label = self.env._("Guardian of a minor member (the minor's own address is not shown)")
            elif kind == KIND_WITHHELD:
                row.contact_label = self.env._("Minor member with no guardian contact on file: do not write to them")
            else:
                row.contact_label = self.env._("The member")
            if contact and contact.active:
                row.contact_name = contact.name
                row.contact_email = (contact.email or "").strip() or (
                    contact.login if "@" in (contact.login or "") else False
                )

    def _contact_partner(self):
        """The partner a reply is addressed to; raises UserError when there is nobody to write to."""
        self.ensure_one()
        user = self.user_id
        kind, contact = (KIND_CLOSED, user.browse()) if not user.active else self._contact_user(user)
        if kind in (KIND_CLOSED, KIND_WITHHELD) or not contact or not contact.active or not self.contact_email:
            raise UserError(self.env._("There is nobody to write to for this member."))
        return contact.partner_id

    # [@ANCHOR: hams_helpdesk:exc_reply_to_member]
    def action_reply_to_member(self):
        """Opens the message composer on the ticket, addressed to the member (or the guardian) and filled with a
        plain note that the site team is working on the problem. A draft only: nothing is sent until the staff
        member presses Send, and the member is not made a follower of the ticket."""
        self.ensure_one()
        partner = self._contact_partner()
        ticket = self.ticket_id
        if not ticket:
            raise UserError(self.env._("This report has no ticket to reply from."))
        return {
            "type": "ir.actions.act_window",
            "name": self.env._("Reply to member"),
            "res_model": "mail.compose.message",
            "view_mode": "form",
            "target": "new",
            "context": {
                "default_composition_mode": "comment",
                "default_model": ticket._name,
                "default_res_ids": ticket.ids,
                "default_partner_ids": partner.ids,
                "default_subject": REPLY_SUBJECT,
                "default_body": REPLY_BODY,
                "default_subtype_xmlid": "mail.mt_comment",
                "mail_post_autofollow": False,
            },
        }

    def action_mailto(self):
        """Opens the staff member's own mail program with the same note (the 'mailto:' link)."""
        self.ensure_one()
        self._contact_partner()
        link = "mailto:{}?subject={}".format(quote(self.contact_email, safe="@"), quote(REPLY_SUBJECT))
        return {"type": "ir.actions.act_url", "url": link, "target": "self"}
