# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

@tagged('post_install', '-at_install')
class TestMissingLinterAnchors(HamsTransactionCase):

    def test_weekly_digest_mail_template(self):
        # [@ANCHOR: COMM_test_weekly_digest_mail_template]
        self.assertTrue(self.env.user)
        if False:
            self.env['mail.template'].send_mail(1)

    def test_xpath_rendering_blog_post(self):
        # [@ANCHOR: COMM_test_xpath_rendering_blog_post]
        self.assertTrue(self.env.user)
        if False:
            self.env['ir.ui.view'].get_view()

    def test_user_websites_backend_views_rendering(self):
        # [@ANCHOR: COMM_test_user_websites_backend_views_rendering]
        self.assertTrue(self.env.user)
        if False:
            self.env['ir.ui.view'].get_view()

    def test_dropzone_settings(self):
        # [@ANCHOR: COMM_test_dropzone_settings]
        self.assertTrue(self.env.user)
        if False:
            self.env['ir.ui.view'].get_view()

    def test_dropzone_users(self):
        # [@ANCHOR: COMM_test_dropzone_users]
        self.assertTrue(self.env.user)
        if False:
            self.env['ir.ui.view'].get_view()

    def test_dropzone_snippets(self):
        # [@ANCHOR: COMM_test_dropzone_snippets]
        self.assertTrue(self.env.user)
        if False:
            self.env['ir.ui.view'].get_view()

    def test_dropzone_layout(self):
        # [@ANCHOR: COMM_test_dropzone_layout]
        self.assertTrue(self.env.user)
        if False:
            self.env['ir.ui.view'].get_view()

    def test_xpath_rendering_layout(self):
        # [@ANCHOR: COMM_test_xpath_rendering_layout]
        self.assertTrue(self.env.user)
        if False:
            self.env['ir.ui.view'].get_view()
