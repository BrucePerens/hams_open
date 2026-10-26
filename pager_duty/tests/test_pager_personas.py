# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.tests.common import HttpCase
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.tests.test_persona_support import PersonaMixin, build_persona, personas


@tagged("post_install", "-at_install")
class TestPagerIncidentPersonas(HamsTransactionCase):
    """The incident rules, run as the users who meet them in production: pager admins have no website."""

    def _incident_on_a_website(self):
        website = self.env["website"].create({"name": "Persona website"})
        return self.env["pager.incident"].create(
            {"source": "persona", "severity": "low", "description": "d", "website_id": website.id}
        )

    # Tests [@ANCHOR: pager_duty:admin_without_website_sees_all]
    @personas(model="pager.incident")
    def test_incident_visibility_follows_the_persona(self, persona):
        """The users the ACL names read an incident of a website; the rest cannot."""
        incident = self._incident_on_a_website()
        visible = bool(incident.with_user(persona.user).has_access("read"))
        if persona.key == "pager_admin":
            self.assertFalse(persona.user.website_id, "a pager admin in production has no website")
            self.assertTrue(visible, "the admin rule hid an incident of a website from an admin with no website")
        if not persona.allowed:
            self.assertFalse(visible, f"{persona.key} must not read an incident")


@tagged("post_install", "-at_install")
class TestPagerIncidentLinkPersonas(HttpCase, PersonaMixin):
    """The "View Pager Duty Incident" link of an incident email, clicked by each kind of recipient.

    A plain HttpCase on purpose: it makes real HTTP requests and needs no browser, while HamsHttpCase starts headless
    Chrome for every test and SKIPS the test, silently, when Chrome does not start."""

    # Tests [@ANCHOR: pager_duty:admin_without_website_sees_all]
    # Tests [@ANCHOR: zero_sudo:persona_login]
    def test_the_email_link_opens_the_incident_for_a_pager_admin_and_not_for_a_portal_member(self):
        """/mail/view lands on the incident for a pager admin, elsewhere for others."""
        website = self.env["website"].create({"name": "Link website"})
        incident = self.env["pager.incident"].create(
            {"source": "link", "severity": "low", "description": "d", "website_id": website.id}
        )
        url = f"/mail/view?model=pager.incident&res_id={incident.id}"
        admin = build_persona(self.env, "pager_admin")
        self.assertFalse(admin.user.website_id)
        self.authenticate_as(admin)
        landed = self.url_open(url, allow_redirects=False).headers.get("Location", "")
        path = landed.split("?")[0].rstrip("/")
        self.assertTrue(path.endswith(f"/{incident.id}"), f"the link did not open the incident: {landed}")
        self.assertNotIn("discuss", landed)
        member = build_persona(self.env, "portal_member")
        self.authenticate_as(member)
        landed = self.url_open(url, allow_redirects=False).headers.get("Location", "")
        self.assertFalse(landed.split("?")[0].rstrip("/").endswith(f"/{incident.id}"), landed)
