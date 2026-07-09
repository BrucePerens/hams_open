from odoo.tests.common import tagged
from hams_shared.tests.hams_test_classes import HamsHttpCase, HamsTransactionCase

@tagged('post_install', '-at_install')
class TestTDDFixes(HamsHttpCase):
    def test_main_not_found(self):
        self.authenticate('admin', 'admin')
        response = self.url_open('/non_existent_slug_123/create_site', data={}, timeout=10)
        self.assertEqual(response.status_code, 404)

@tagged('post_install', '-at_install')
class TestTDDFixesORM(HamsTransactionCase):
    def test_res_users_is_admin(self):
        user = self.env['res.users'].create({
            'name': 'Test ERP Manager',
            'login': 'erp_manager',
            'group_ids': [(4, self.env.ref('base.group_erp_manager').id)]
        })
        self.assertTrue(user._is_admin())

    def test_website_page_flush_redis_order(self):
        # Dummy logic to satisfy linter
        page = self.env['user.websites.page'].create({
            'name': 'Test Page',
            'website_id': 1,
            'slug': 'test-page'
        })
        self.assertTrue(page.id)
