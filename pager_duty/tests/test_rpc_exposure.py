# SPDX-License-Identifier: AGPL-3.0-or-later

"""`/web/dataset/call_kw` reaches every public method. These methods act through a service account (or return what it reads), and
only Python and cron call them, so they are `@api.private`: an RPC call is refused whatever the caller's groups."""
from odoo.exceptions import AccessError
from odoo.service.model import get_public_method
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestRpcRefusesServiceOnlyMethods(HamsTransactionCase):
    def test_service_only_methods_cannot_be_called_over_rpc(self):
        for model, name in (
            ('pager.incident', 'action_escalate_unacknowledged'),
        ):
            with self.subTest(model=model, method=name):
                with self.assertRaises(AccessError):
                    get_public_method(self.env[model], name)


@tagged("post_install", "-at_install")
class TestAutoResolveCallerCheck(HamsTransactionCase):
    """`auto_resolve_incidents` is called over RPC by the pager daemon, so it stays public; it closes incidents, so only a
    service account, the superuser environment or a pager administrator may call it."""

    def _member(self):
        return self.env["res.users"].create({
            "name": "Ordinary Member", "login": "pager_ordinary_member@example.com",
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })

    def test_an_ordinary_member_cannot_resolve_incidents(self):
        with self.assertRaises(AccessError):
            self.env["pager.incident"].with_user(self._member()).auto_resolve_incidents("rpc_exposure_source")

    def test_the_pager_service_account_can(self):
        uid = self.env["zero_sudo.security.utils"]._get_service_uid("pager_duty.user_pager_service_internal")
        self.assertTrue(self.env["pager.incident"].with_user(uid).auto_resolve_incidents("rpc_exposure_source"))

    def test_the_superuser_environment_can(self):
        self.assertTrue(self.env.su)
        self.env["pager.incident"].auto_resolve_incidents("rpc_exposure_source")
