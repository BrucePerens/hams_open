from odoo.tests import HttpCase, tagged

@tagged('post_install', '-at_install', 'ui', 'integration')
class TestHelpdeskTours(HttpCase):

    def test_01_helpdesk_operator_tour(self):
        """Simulates an operator logging in, viewing their shift, and executing a handoff."""
        # [@ANCHOR: test_helpdesk_operator_tour]
        # Note: In a real CI environment, we ensure the 'admin' user has helpdesk manager rights.
        self.start_tour("/odoo", "helpdesk_operator_tour", login="admin")

    def test_02_helpdesk_portal_tour(self):
        """Simulates a customer checking their ticket status on the external portal."""
        # Assuming the standard 'portal' demo user exists
        self.start_tour("/", "helpdesk_portal_tour", login="portal")
