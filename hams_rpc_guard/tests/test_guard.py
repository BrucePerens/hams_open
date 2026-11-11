# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import ipaddress
import xmlrpc.client

from odoo.addons.rpc.controllers import json2
from odoo.exceptions import AccessError
from odoo.service import model as service_model
from odoo.tests import tagged
from odoo.addons.zero_sudo.models.security_utils import ZeroSudoSecurityUtils
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase
from odoo.addons.zero_sudo.tests.test_persona_support import build_persona

from types import SimpleNamespace

from werkzeug.datastructures import Headers

from odoo.addons.hams_rpc_guard import endpoint_gate, exposure, patch as guard, policy


class GuardCase:
    """Shared setup: the parameters as a dict, the declarations as a settable `Exposure`, and no leftover counters."""

    def setUp(self):
        super().setUp()
        self.values = {}
        self.declared = exposure.Exposure()

        def read(utils, key, default=None):
            value = self.values.get(key)
            return value if value is not None else default
        self.safe_patch_object(ZeroSudoSecurityUtils, "_get_system_param", new=read)
        self.safe_patch_object(exposure, "load_exposure", new=lambda cr, registry: self.declared)
        policy._counts.clear()
        self.addCleanup(policy._counts.clear)

    def _own(self, model_name, method):
        """Make every addon that defines `model_name.method` count as ours, so the method is judged as one of ours."""
        cls = type(self.env[model_name])
        self.declared.modules.update(c.__dict__["_module"] for c in cls.mro() if method in c.__dict__ and "_module" in c.__dict__)

    def _as_rpc(self):
        token = guard._external.set(True)
        self.addCleanup(guard._external.reset, token)


@tagged("post_install", "-at_install")
class TestWrapper(GuardCase, HamsTransactionCase):
    """The wrapper installed over `get_public_method`, as the three call paths use it."""
    # Tests [@ANCHOR: hams_rpc_guard:call_is_rpc]
    # Tests [@ANCHOR: hams_rpc_guard:module_is_managed]
    # Tests [@ANCHOR: hams_rpc_guard:describe_call]
    # Tests [@ANCHOR: hams_rpc_guard:check_rpc_call]
    # Tests [@ANCHOR: hams_rpc_guard:guard_get_public_method]
    # Tests [@ANCHOR: hams_rpc_guard:guard_execute_cr]
    # Tests [@ANCHOR: hams_rpc_guard:install]
    # Tests [@ANCHOR: hams_rpc_guard:post_load_hook]

    METHOD = "address_get"  # defined by res.partner in `base`, and not a standard ORM method

    def test_the_module_is_installed_over_the_core_functions(self):
        self.assertTrue(service_model.get_public_method.__dict__.get(guard.WRAPPED))
        self.assertTrue(service_model.execute_cr.__dict__.get(guard.WRAPPED))
        self.assertIs(json2.get_public_method, service_model.get_public_method)
        guard.install()  # idempotent: no double wrapping
        self.assertNotIn(guard.WRAPPED, service_model.get_public_method.__wrapped__.__dict__)

    def test_python_code_is_never_judged(self):
        self.assertTrue(callable(service_model.get_public_method(self.env["res.partner"], self.METHOD)))
        self.assertEqual(policy.verdict_counts(), {})

    def test_core_refusals_still_come_first(self):
        self._as_rpc()
        with self.assertRaises(AccessError):
            service_model.get_public_method(self.env["res.partner"], "_search")

    def setUp(self):
        super().setUp()
        self._own("res.partner", self.METHOD)

    def test_internal_user_a_managed_method_is_refused_until_listed(self):
        self._as_rpc()
        staff = build_persona(self.env, "internal_staff").env["res.partner"]
        with self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError):
            service_model.get_public_method(staff, self.METHOD)
        self.declared.internal.add(("res.partner", self.METHOD))
        self.assertTrue(callable(service_model.get_public_method(staff, self.METHOD)))

    def test_stock_methods_of_an_internal_user_keep_core_behaviour(self):
        self._as_rpc()
        staff = build_persona(self.env, "internal_staff").env["res.partner"]
        self.assertTrue(callable(service_model.get_public_method(staff, "search_read")))
        self.assertEqual(policy.verdict_counts(), {})

    def test_report_mode_serves_the_call_and_counts_it(self):
        self._as_rpc()
        self.values[policy.MODE_PARAM] = "report"
        staff = build_persona(self.env, "internal_staff").env["res.partner"]
        with self.assertLogs(policy._logger, "WARNING"):
            self.assertTrue(callable(service_model.get_public_method(staff, self.METHOD)))
        self.assertEqual(
            policy.verdict_counts(), {("report", "internal", "res.partner." + self.METHOD, "managed-not-listed"): 1})

    def test_off_mode_judges_nothing(self):
        self._as_rpc()
        self.values[policy.MODE_PARAM] = "off"
        staff = build_persona(self.env, "internal_staff").env["res.partner"]
        self.assertTrue(callable(service_model.get_public_method(staff, self.METHOD)))
        self.assertEqual(policy.verdict_counts(), {})

    def test_a_portal_user_gets_only_the_listed_pairs(self):
        self._as_rpc()
        member = build_persona(self.env, "portal_member").env["res.partner"]
        with self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError):
            service_model.get_public_method(member, "search_read")
        self.declared.portal.add(("res.partner", "search_read"))
        self.assertTrue(callable(service_model.get_public_method(member, "search_read")))

    def test_the_public_user_gets_only_the_listed_pairs(self):
        self._as_rpc()
        visitor = build_persona(self.env, "public").env["res.partner"]
        with self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError):
            service_model.get_public_method(visitor, "search_read")

    def test_a_daemon_is_limited_to_its_own_entry(self):
        self._as_rpc()
        daemon = build_persona(self.env, "internal_staff")
        self.safe_patch_object(type(self.env["ir.http"]), "_is_service_account_cached", new=lambda cls, uid: uid == daemon.user.id)
        self.declared.clients[daemon.user.login] = {("res.partner", "read")}
        model = daemon.env["res.partner"]
        self.assertTrue(callable(service_model.get_public_method(model, "read")))
        with self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError):
            service_model.get_public_method(model, "search_read")  # not in this daemon's entry

    def test_a_portal_type_service_account_is_a_daemon_not_a_portal_visitor(self):
        """Found by running the backup module's tests with the guard installed: the backup worker's service account is a
        portal-type user, as most service accounts are, and was refused as a strict portal user."""
        self._as_rpc()
        daemon = build_persona(self.env, "portal_member")
        self.safe_patch_object(type(self.env["ir.http"]), "_is_service_account_cached", new=lambda cls, uid: uid == daemon.user.id)
        self.declared.clients[daemon.user.login] = {("res.partner", "read")}
        model = daemon.env["res.partner"]
        self.assertTrue(callable(service_model.get_public_method(model, "read")))
        with self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError):
            service_model.get_public_method(model, "search_read")
        self.assertIn(("enforce", "service", "res.partner.search_read", "client-not-listed"), policy.verdict_counts())

    def test_a_certificate_subject_is_believed_only_from_a_trusted_proxy(self):
        # Tests [@ANCHOR: hams_rpc_guard:certificate_identity]
        utils = self.env["zero_sudo.security.utils"]
        loopback = str(ipaddress.IPv4Address(0x7F000001))
        request = SimpleNamespace(httprequest=SimpleNamespace(
            environ={"REMOTE_ADDR": loopback}, headers=Headers([("X-Client-Cert-Subject", " CN=fcc-sync ")])))
        self.safe_patch_object(guard, "request", new=request)
        self.assertEqual(guard.certificate_identity(utils), "", "no header named: off by default")
        self.values[policy.CLIENT_CERT_HEADER_PARAM] = "X-Client-Cert-Subject"
        self.assertEqual(guard.certificate_identity(utils), "", "no trusted proxy configured")
        self.values[endpoint_gate.TRUSTED_PROXIES_PARAM] = loopback
        self.assertEqual(guard.certificate_identity(utils), "cert:CN=fcc-sync")
        request.httprequest.headers = Headers()
        self.assertEqual(guard.certificate_identity(utils), "", "a request without the header")

    def test_an_unreadable_parameter_fails_closed_to_enforce(self):
        self._as_rpc()

        class Broken(dict):
            def get(self, key, default=None):
                raise RuntimeError("cache down")
        staff = build_persona(self.env, "internal_staff").env["res.partner"]  # other addons read parameters while creating a user
        self.values = Broken()
        with self.assertLogs(guard._logger, "ERROR"), self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError):
            service_model.get_public_method(staff, self.METHOD)


@tagged("post_install", "-at_install")
class TestSecretBugClasses(GuardCase, HamsTransactionCase):
    """The tonight-bug classes of the default-deny to-do, pinned against the installed registry."""

    def _staff(self, model_name):
        self._as_rpc()
        return build_persona(self.env, "internal_staff").env[model_name]

    def test_a_settings_get_values_override_of_ours_is_not_rpc_callable(self):
        # The hams_s3 bug: `res.config.settings.get_values` returned a secret to any signed-in caller over call_kw.
        # Every installed override of ours must be @api.private (refused over RPC); the stock one is only reachable by group_system.
        staff = self._staff("res.config.settings")
        ours = [c for c in type(staff).mro() if "get_values" in c.__dict__ and "_module" in c.__dict__
                and guard._module_is_managed(c.__dict__["_module"], policy.DEFAULT_MANAGED_AUTHORS)]
        if ours:
            with self.assertRaises(AccessError):
                service_model.get_public_method(staff, "get_values")

    def test_service_identity_helpers_are_not_rpc_callable(self):
        # Methods that run as a service account and trust the caller must never be reachable over call_kw.
        staff = self._staff("zero_sudo.security.utils")
        for name in ("_get_service_env", "_get_service_uid", "_erase_via_service_account"):
            with self.assertRaises(AccessError, msg=name):
                service_model.get_public_method(staff, name)

    def test_a_public_model_method_of_ours_needs_an_exposure_entry(self):
        # The ham_nets bug: a public @api.model import method callable by any signed-in user.
        staff = self._staff("res.partner")
        self._own("res.partner", "address_get")
        with self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError):
            service_model.get_public_method(staff, "address_get")


@tagged("post_install", "-at_install")
class TestOverHttp(GuardCase, HamsHttpCase):
    """The same decisions through the real routes: the web client's call_kw, JSON-2 and XML-RPC execute_kw."""
    # Tests [@ANCHOR: hams_rpc_guard:call_is_rpc]
    # Tests [@ANCHOR: hams_rpc_guard:guard_execute_cr]

    def _call_kw(self, model, method, args=None, kwargs=None):
        payload = {"jsonrpc": "2.0", "method": "call", "params": {
            "model": model, "method": method, "args": args or [], "kwargs": kwargs or {}}}
        return self.url_open("/web/dataset/call_kw/%s/%s" % (model, method), json=payload).json()

    def test_a_portal_member_over_call_kw_gets_only_the_baseline(self):
        persona = build_persona(self.env, "portal_member")
        self.authenticate_as(persona)
        self.declared.portal.add(("res.users", "read"))
        allowed = self._call_kw("res.users", "read", [[persona.user.id], ["name"]])
        self.assertIn("result", allowed, allowed)
        refused = self._call_kw("res.partner", "search_read", [[]], {"fields": ["name"]})
        self.assertNotIn("result", refused)
        self.assertIn("cannot be called remotely", refused["error"]["data"]["message"])

    def test_an_internal_user_keeps_the_web_client(self):
        persona = build_persona(self.env, "admin")
        self.authenticate_as(persona)
        result = self._call_kw("res.partner", "search_read", [[]], {"fields": ["name"], "limit": 1})
        self.assertIn("result", result, result)

    def test_xmlrpc_execute_kw_is_judged_too(self):
        persona = build_persona(self.env, "portal_member")
        body = xmlrpc.client.dumps(
            (self.env.cr.dbname, persona.user.id, persona.password, "res.partner", "search_read", [[]], {"fields": ["name"]}),
            "execute_kw")
        response = self.url_open("/xmlrpc/2/object", data=body, headers={"Content-Type": "text/xml"})
        self.assertIn("cannot be called remotely", response.text)
        self.assertNotIn("Traceback", response.text)
