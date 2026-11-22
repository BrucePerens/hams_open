# Copyright © HAMS project. AGPL-3.0-or-later.
# -*- coding: utf-8 -*-
import re
from datetime import datetime, timedelta, timezone

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestSecurityTxt(HamsHttpCase):
    # Tests [@ANCHOR: edge_cache_security_txt_security_txt]
    def test_security_txt_served(self):
        # [@ANCHOR: COMM_test_security_txt_served]
        # Tests [@ANCHOR: COMM_security_txt_route]
        response = self.url_open("/.well-known/security.txt")
        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.headers["Content-Type"].startswith("text/plain"))
        text = response.text
        self.assertIn("Contact: mailto:security@hams.com\n", text)
        self.assertIn("Preferred-Languages: en\n", text)
        expires = re.search(r"^Expires: (\S+)$", text, re.M).group(1)
        when = datetime.strptime(expires, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
        now = datetime.now(timezone.utc)
        self.assertGreater(when, now + timedelta(days=30))
        self.assertLess(when, now + timedelta(days=365))
        self.assertEqual(response.headers.get("Cloudflare-CDN-Cache-Control"), "max-age=86400")
        self.assertNotIn("X-Cloudflare-Edge-Cacheable", response.headers)
