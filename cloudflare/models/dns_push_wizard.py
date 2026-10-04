# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import api, fields, models, _
from odoo.exceptions import UserError


class CloudflareDnsPushWizard(models.TransientModel):
    """Two steps: show the plan (reads only), then apply exactly that plan."""

    _name = "cloudflare.dns.push.wizard"
    _description = "Cloudflare DNS Push"

    name = fields.Char(string="Name", default=lambda self: self._description)
    record_ids = fields.Many2many(
        "cloudflare.dns.record",
        string="Records",
        default=lambda self: self.env["cloudflare.dns.record"].browse(self.env.context.get("active_ids") or []).exists(),
    )
    state = fields.Selection([("draft", "Draft"), ("planned", "Planned"), ("done", "Done")], default="draft")
    plan_text = fields.Text(string="Plan", readonly=True)
    plan_hash = fields.Char(readonly=True)
    plan_writes = fields.Integer(readonly=True)
    result_text = fields.Text(string="Result", readonly=True)

    def _reopen(self):
        return {
            "type": "ir.actions.act_window",
            "res_model": self._name,
            "res_id": self.id,
            "view_mode": "form",
            "target": "new",
        }

    # [@ANCHOR: cloudflare:COMM_dns_push_wizard_plan]
    def action_plan(self):
        self.ensure_one()
        records = self.record_ids or self.env["cloudflare.dns.record"].search([], limit=10000)
        plan = records.dns_push_plan()
        writes = sum(1 for e in plan["entries"] if e["action"] in ("create", "update", "adopt"))
        text = plan["text"]
        if plan["problems"]:
            text = "REFUSED: " + "; ".join(plan["problems"]) + "\n\n" + text
        self.write({"state": "planned", "plan_text": text, "plan_hash": plan["hash"], "plan_writes": writes})
        return self._reopen()

    # [@ANCHOR: cloudflare:COMM_dns_push_wizard_apply]
    def action_apply(self):
        self.ensure_one()
        if self.state != "planned" or not self.plan_hash:
            raise UserError(_("Make the plan first."))
        records = self.record_ids or self.env["cloudflare.dns.record"].search([], limit=10000)
        results = records.dns_push_apply(self.plan_hash)
        self.write({"state": "done", "result_text": "\n".join(results) or _("Nothing to do.")})
        return self._reopen()
