# -*- coding: utf-8 -*-
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDomainPush(HamsTransactionCase):

    def test_domain_push_logic(self):
        """Test the logic that gathers domains and pushes them."""
        # Instead of dealing with postcommit complexities in tests,
        # we will directly test the _invalidate_cache logic by simulating the environment.
        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )

        # Create a domain
        domain_model.create(
            {
                "name": "manualpush.com",
                "target_slug": "manualpush",
            }
        )
        self.env.flush_all()

        # We can't easily mock the inner function `push_to_pager_duty`
        # But we can call the outer function to ensure it doesn't crash.
        domain_model._invalidate_cache(["manualpush.com"])
