from odoo.tests import tagged
from odoo.addons.hams_test.common import HamsHttpCase

@tagged('post_install', '-at_install', 'ui', 'integration')
class TestHelpdeskTours(HamsHttpCase):

    def test_01_helpdesk_operator_tour(self):
        """Simulates an operator logging in, viewing their shift, and executing a handoff."""
        # [@ANCHOR: test_helpdesk_operator_tour]
        # Note: In a real CI environment, we ensure the 'admin' user has helpdesk manager rights.
        self.start_tour("/odoo?debug=1", "helpdesk_operator_tour", login="admin")

    def test_02_helpdesk_portal_tour(self):
        """Simulates a customer checking their ticket status on the external portal."""
        # Create a portal user for the tour to ensure it exists
        portal_user = self.env['res.users'].search([('login', '=', 'portal_test')], limit=1)
        if not portal_user:
            portal_user = self.env['res.users'].create({
                'name': 'Portal Test User',
                'login': 'portal_test',
                'password': 'portal_test_password',
                'group_ids': [(6, 0, [self.env.ref('base.group_portal').id])]
            })

        # Create a ticket for the portal user so the tour has something to click on
        self.env['hams_helpdesk.ticket'].create({
            'name': 'Tour Test Ticket',
            'partner_id': portal_user.partner_id.id,
        })

        self.start_tour("/?debug=1", "helpdesk_portal_tour", login="portal_test")
