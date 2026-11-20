# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Problem text holding markup stays inert on the portal ticket page and in the assignment mail body."""
from lxml import html

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged

PAYLOAD = '<img src=x onerror=alert(1)>"><script>alert(2)</script><a href="javascript:alert(3)">x</a>'


@tagged("post_install", "-at_install")
class TestPortalXss(HamsHttpCase):

    def test_payload_is_inert_on_portal_page_and_in_mail_body(self):
        # Tests [@ANCHOR: hams_helpdesk:assignment_notice_body]
        member = self.env["res.users"].create(
            {"name": "Xss Member " + PAYLOAD, "login": "helpdesk_xss_member", "password": "helpdesk_xss_member",
             "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])]}
        )
        ticket = self.env["hams_helpdesk.ticket"].create(
            {"name": "Problem " + PAYLOAD, "description": PAYLOAD, "callsign": PAYLOAD[:40],
             "partner_id": member.partner_id.id}
        )
        self.authenticate("helpdesk_xss_member", "helpdesk_xss_member")
        for path in ("/my/ticket/%s" % ticket.id, "/my/tickets"):
            doc = html.fromstring(self.url_open(path).content)
            self.assertFalse(doc.xpath("//img[@src='x']"), path)
            self.assertFalse(doc.xpath("//*[@onerror]"), path)
            self.assertFalse(doc.xpath("//a[starts-with(@href, 'javascript:')]"), path)
            self.assertFalse([s for s in doc.xpath("//script[not(contains(@type, 'json'))]") if "alert(2)" in (s.text or "")], path)
        body = html.fromstring("<div>%s</div>" % ticket._assignment_notice_body())
        self.assertFalse(body.xpath("//img"))
        self.assertFalse(body.xpath("//script"))
        self.assertFalse(body.xpath("//*[@onerror]"))
        self.assertFalse(body.xpath("//a[starts-with(@href, 'javascript:')]"))
