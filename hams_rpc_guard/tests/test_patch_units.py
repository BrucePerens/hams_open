# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free tests of `hams_rpc_guard.patch` against fake models, requests and parameters, added to kill the
mutation survivors of the second mutation pass (2026-11-13)."""
import logging
from types import SimpleNamespace

from odoo.addons.rpc.controllers import json2
from odoo.service import model as service_model
from odoo.tests.common import BaseCase, tagged
from odoo.addons.zero_sudo.tests.common import SafePatchMixin

from odoo.addons.hams_rpc_guard import endpoint_gate as gate, exposure, patch as guard, policy


class _Headers(dict):
    def get(self, key, default=None):
        return super().get(key, default)


def _request(path="/x", environ=None, headers=None):
    return SimpleNamespace(httprequest=SimpleNamespace(path=path, environ=environ or {}, headers=_Headers(headers or {})))


class _Utils:
    def __init__(self, values, fail=False):
        self.values, self.fail, self.asked = values, fail, []

    def _get_system_param(self, key, default=None):
        self.asked.append((key, default))
        if self.fail:
            raise ValueError("unreadable")
        return self.values.get(key, default)


class _Env(dict):
    def __init__(self, utils, user, service_ids=()):
        super().__init__()
        self["zero_sudo.security.utils"] = utils
        self["ir.http"] = SimpleNamespace(_is_service_account_cached=lambda uid: uid in service_ids)
        self.user = user
        self.cr = "cr"
        self.registry = "registry"


def _user(uid=5, login="u", public=False, internal=True):
    return SimpleNamespace(id=uid, login=login, _is_public=lambda: public, _is_internal=lambda: internal)


@tagged("post_install", "-at_install")
class TestPatchUnits(SafePatchMixin, BaseCase):
    # Tests [@ANCHOR: hams_rpc_guard:call_is_rpc]
    # Tests [@ANCHOR: hams_rpc_guard:module_is_managed]
    # Tests [@ANCHOR: hams_rpc_guard:describe_call]
    # Tests [@ANCHOR: hams_rpc_guard:certificate_identity]
    # Tests [@ANCHOR: hams_rpc_guard:unix_identity]
    # Tests [@ANCHOR: hams_rpc_guard:check_rpc_call]
    # Tests [@ANCHOR: hams_rpc_guard:guard_get_public_method]
    # Tests [@ANCHOR: hams_rpc_guard:guard_execute_cr]
    # Tests [@ANCHOR: hams_rpc_guard:install]

    def _req(self, **kwargs):
        self.safe_patch_object(guard, "request", new=_request(**kwargs))

    # ---- call_is_rpc -------------------------------------------------------------------------------------------

    def test_call_is_rpc_paths(self):
        for path, expected in (("/web/dataset/call_kw/res.partner/x", True), ("/web/dataset/call_button", True),
                               ("/json/2/res.partner/x", True), ("/web/dataset/other", False), ("/other", False),
                               ("/x/web/dataset/call_kw", False), ("", False), (None, False)):
            self._req(path=path)
            self.assertIs(guard.call_is_rpc(), expected, path)

    def test_call_is_rpc_external_flag_wins_without_a_request(self):
        class NoRequest:
            def __bool__(self):
                return False
            httprequest = SimpleNamespace(path="/json/2/a.b/c")
        self.safe_patch_object(guard, "request", new=NoRequest())
        self.assertFalse(guard.call_is_rpc())  # falsy request is not consulted
        token = guard._external.set(True)
        try:
            self.assertTrue(guard.call_is_rpc())
        finally:
            guard._external.reset(token)
        self.assertFalse(guard.call_is_rpc())

    def test_call_is_rpc_runtime_error_is_not_rpc(self):
        class Raising:
            @property
            def httprequest(self):
                raise RuntimeError("no request")
        self.safe_patch_object(guard, "request", new=Raising())
        self.assertIs(guard.call_is_rpc(), False)

    # ---- module_is_managed -------------------------------------------------------------------------------------

    def test_module_is_managed(self):
        guard._module_is_managed.cache_clear()
        self.addCleanup(guard._module_is_managed.cache_clear)
        manifests = {"ours": {"author": "Hams Inc"}, "theirs": {"author": "Odoo S.A."}, "empty": {}, "none": None}
        self.safe_patch_object(exposure, "manifest_of", new=manifests.get)
        self.assertTrue(guard._module_is_managed("ours", ("hams",)))
        self.assertFalse(guard._module_is_managed("theirs", ("hams",)))
        self.assertTrue(guard._module_is_managed("theirs", ("odoo",)))  # the authors argument is honoured
        self.assertFalse(guard._module_is_managed("empty", ("hams",)))
        self.assertFalse(guard._module_is_managed("none", ("hams",)))
        self.assertEqual(guard._module_is_managed.cache_info().maxsize, 4096)

    # ---- describe_call -----------------------------------------------------------------------------------------

    def _model(self, user, service_ids=(), chain=(("Mixin", "base", "mixin.thing"), ("Concrete", "mine", "the.thing")), name="go"):
        classes = []
        base = object
        for cname, module, _name in chain:
            body = {"_module": module, "_name": _name, name: lambda self: None}
            base = type(cname, (base,), body)
            classes.append(base)
        model_class = type("Model", (base,), {})
        model = model_class()
        model._name = "the.thing"
        model.env = _Env(_Utils({}), user, service_ids)
        return model

    def test_describe_call_user_classes(self):
        authors = ("hams",)
        exp = exposure.Exposure()
        guard._module_is_managed.cache_clear()
        self.addCleanup(guard._module_is_managed.cache_clear)
        self.safe_patch_object(exposure, "manifest_of", new=lambda module: {"author": "hams"} if module in ("mine", "base") else None)
        cases = (
            (_user(public=True, internal=False), (), policy.USER_PUBLIC, ""),
            (_user(uid=9, login="svc", internal=False), (9,), policy.USER_SERVICE, "svc"),
            (_user(uid=9, login="svc", internal=True), (9,), policy.USER_SERVICE, "svc"),
            (_user(login="por", internal=False), (), policy.USER_PORTAL, ""),
            (_user(login="int", internal=True), (), policy.USER_INTERNAL, ""),
        )
        for user, service_ids, user_class, login in cases:
            call = guard.describe_call(self._model(user, service_ids), "go", authors, exp, cert="cert:x")
            self.assertEqual((call.user_class, call.login), (user_class, login))
            self.assertEqual(call.cert, "cert:x")
            self.assertEqual(call.model, "the.thing")
            self.assertEqual(call.method, "go")
        self.assertEqual(guard.describe_call(self._model(_user()), "go", authors, exp).cert, "")

    def test_describe_call_defining_models_managed_and_standard(self):
        exp = exposure.Exposure()
        guard._module_is_managed.cache_clear()
        self.addCleanup(guard._module_is_managed.cache_clear)
        self.safe_patch_object(exposure, "manifest_of", new=lambda module: {"author": "hams"} if module == "mine" else {"author": "odoo"})
        model = self._model(_user())
        call = guard.describe_call(model, "go", ("hams",), exp)
        self.assertEqual(call.defining_models, ("the.thing", "mixin.thing"))  # most derived first, deduplicated
        self.assertFalse(call.managed)  # the mixin's module "base" is not ours
        exp.modules.add("base")
        self.assertTrue(guard.describe_call(model, "go", ("hams",), exp).managed)  # declared modules count
        self.assertFalse(call.orm_standard)
        self.assertEqual(call.marks, ())
        # a name nobody defines: not managed
        none = guard.describe_call(model, "nobody_defines_this", ("hams",), exp)
        self.assertFalse(none.managed)
        self.assertEqual(none.defining_models, ())
        # a class with no _module makes the method not ours; a class with no _name contributes ""
        nomod = self._model(_user(), chain=(("Only", None, None),))
        called = guard.describe_call(nomod, "go", ("hams",), exp)
        self.assertFalse(called.managed)
        self.assertEqual(called.defining_models, ("",))
        # a standard ORM name is flagged
        std = self._model(_user(), chain=(("Only", "mine", "x.y"),), name="write")
        self.assertTrue(guard.describe_call(std, "write", ("hams",), exp).orm_standard)

    def test_describe_call_all_must_be_ours(self):
        exp = exposure.Exposure()
        guard._module_is_managed.cache_clear()
        self.addCleanup(guard._module_is_managed.cache_clear)
        self.safe_patch_object(exposure, "manifest_of", new=lambda module: {"author": "hams"})
        model = self._model(_user(), chain=(("A", "m1", "a.a"), ("B", "m2", "b.b")))
        self.assertTrue(guard.describe_call(model, "go", ("hams",), exp).managed)
        self.safe_patch_object(exposure, "manifest_of", new=lambda module: {"author": "hams"} if module == "m1" else {"author": "x"})
        guard._module_is_managed.cache_clear()
        self.assertFalse(guard.describe_call(model, "go", ("hams",), exp).managed)

    def test_describe_call_marks(self):
        exp = exposure.Exposure()
        self.safe_patch_object(exposure, "manifest_of", new=lambda module: None)
        model = self._model(_user())
        self.safe_patch_object(exposure, "decorator_marks", new=lambda cls, name: [{"mark": name}])
        self.assertEqual(guard.describe_call(model, "go", ("hams",), exp).marks, ({"mark": "go"},))

    # ---- certificate_identity ----------------------------------------------------------------------------------

    def _cert_utils(self, header="X-Cert", proxies="127.0.0.0/8"):
        return _Utils({policy.CLIENT_CERT_HEADER_PARAM: header, "hams_rpc_guard.trusted_proxies": proxies})

    def test_certificate_identity(self):
        self._req(environ={"REMOTE_ADDR": "127.0.0.2"}, headers={"X-Cert": "  CN=bob "})
        utils = self._cert_utils()
        self.assertEqual(guard.certificate_identity(utils), "cert:CN=bob")
        self.assertEqual(utils.asked[0], (policy.CLIENT_CERT_HEADER_PARAM, ""))
        self.assertEqual(utils.asked[1], ("hams_rpc_guard.trusted_proxies", ""))
        self.assertEqual(guard.certificate_identity(self._cert_utils(header="")), "")
        self.assertEqual(guard.certificate_identity(self._cert_utils(proxies="10.0.0.0/8")), "")
        self._req(environ={"REMOTE_ADDR": "8.8.8.8"}, headers={"X-Cert": "CN=bob"})
        self.assertEqual(guard.certificate_identity(self._cert_utils(proxies="0.0.0.0/0")), "")
        self._req(environ={"REMOTE_ADDR": "127.0.0.2"}, headers={})
        self.assertEqual(guard.certificate_identity(self._cert_utils()), "")
        self._req(environ={"REMOTE_ADDR": "127.0.0.2"}, headers={"X-Cert": "   "})
        self.assertEqual(guard.certificate_identity(self._cert_utils()), "")
        self._req(environ={"REMOTE_ADDR": "127.0.0.2"}, headers={"Other": "CN=bob"})
        self.assertEqual(guard.certificate_identity(self._cert_utils()), "")

    def test_certificate_identity_without_request(self):
        class NoRequest:
            def __bool__(self):
                return False
            httprequest = _request(environ={"REMOTE_ADDR": "127.0.0.2"}, headers={"X-Cert": "CN=a"}).httprequest
        self.safe_patch_object(guard, "request", new=NoRequest())
        self.assertEqual(guard.certificate_identity(self._cert_utils()), "")

    # ---- unix_identity -----------------------------------------------------------------------------------------

    def test_unix_identity(self):
        class NoRequest:
            def __bool__(self):
                return False
            httprequest = SimpleNamespace(environ={gate.UNIX_PEER_ENVIRON_KEY: {"user": "x", "uid": 1}})
        self.safe_patch_object(guard, "request", new=NoRequest())
        self.assertEqual(guard.unix_identity(), ("", None))
        self._req(environ={})
        self.assertEqual(guard.unix_identity(), ("", None))
        self._req(environ={gate.UNIX_LOGINS_ENVIRON_KEY: frozenset({"a"})})
        self.assertEqual(guard.unix_identity(), ("", None))  # logins without a peer mean nothing
        logins = frozenset({"a", "b"})
        self._req(environ={gate.UNIX_PEER_ENVIRON_KEY: {"user": "alice", "uid": 1000}, gate.UNIX_LOGINS_ENVIRON_KEY: logins})
        self.assertEqual(guard.unix_identity(), ("unix:alice", logins))
        self._req(environ={gate.UNIX_PEER_ENVIRON_KEY: {"user": "", "uid": 1234}})
        self.assertEqual(guard.unix_identity(), ("unix:uid:1234", None))
        self._req(environ={gate.UNIX_PEER_ENVIRON_KEY: {"uid": 7}})
        self.assertEqual(guard.unix_identity(), ("unix:uid:7", None))

    # ---- check_rpc_call ----------------------------------------------------------------------------------------

    def _check(self, values=None, fail=False, environ=None, login="u", cert="", describe_cert_expected=None):
        """Run check_rpc_call with policy/describe stubbed; return what reached them."""
        seen = {}
        utils = _Utils(values or {}, fail=fail)
        user = _user(login=login)
        model = SimpleNamespace(env=_Env(utils, user), _name="a.b")
        env_exposure = exposure.Exposure()
        self.safe_patch_object(exposure, "load_exposure", new=lambda cr, registry: seen.setdefault("load", (cr, registry)) and env_exposure)
        call = policy.Call("a.b", "go", policy.USER_INTERNAL)

        def describe(m, name, authors, exp, cert_arg=""):
            seen["describe"] = (m, name, authors, exp is env_exposure, cert_arg)
            return call
        self.safe_patch_object(guard, "describe_call", new=describe)
        self.safe_patch_object(guard, "certificate_identity", new=lambda u: cert)

        def decide(c, exp, strict):
            seen["decide"] = (c, exp is env_exposure, strict)
            return policy.ALLOW, "decided"
        self.safe_patch_object(policy, "decide", new=decide)
        self.safe_patch_object(policy, "apply_verdict", new=lambda c, v, r, m: seen.update(apply=(c, v, r, m)))
        self._req(environ=environ or {})
        guard.check_rpc_call(model, "go")
        seen["model"], seen["call"], seen["utils"] = model, call, utils
        return seen

    def test_check_rpc_call_reads_parameters_and_decides(self):
        seen = self._check({policy.MODE_PARAM: "report", policy.STRICT_STOCK_PARAM: "true", policy.MANAGED_AUTHORS_PARAM: "Foo, Bar"},
                           cert="cert:me")
        self.assertEqual(seen["load"], ("cr", "registry"))
        self.assertEqual(seen["describe"][1:], ("go", ("Foo", "Bar"), True, "cert:me"))
        self.assertEqual(seen["decide"], (seen["call"], True, True))
        self.assertEqual(seen["apply"], (seen["call"], policy.ALLOW, "decided", policy.MODE_REPORT))
        keys = dict(seen["utils"].asked)
        self.assertEqual(keys[policy.MODE_PARAM], policy.DEFAULT_MODE)
        self.assertEqual(keys[policy.STRICT_STOCK_PARAM], "")
        self.assertEqual(keys[policy.MANAGED_AUTHORS_PARAM], "")

    def test_check_rpc_call_defaults_and_off(self):
        seen = self._check({})
        self.assertEqual(seen["apply"][3], policy.MODE_ENFORCE)
        self.assertEqual(seen["describe"][2], policy.DEFAULT_MANAGED_AUTHORS)
        self.assertFalse(seen["decide"][2])
        seen = self._check({policy.MODE_PARAM: "off"})
        self.assertNotIn("apply", seen)
        self.assertNotIn("load", seen)
        self.assertNotIn("decide", seen)

    def test_check_rpc_call_unreadable_parameters_fall_back_to_strict(self):
        with self.assertLogs(guard._logger, level=logging.ERROR) as logs:
            seen = self._check(fail=True, cert="cert:ignored")
        self.assertIn("Could not read the RPC guard parameters", logs.output[0])
        self.assertEqual(seen["apply"][3], policy.MODE_ENFORCE)
        self.assertEqual(seen["describe"][2:], (policy.DEFAULT_MANAGED_AUTHORS, seen["describe"][3], ""))
        self.assertFalse(seen["decide"][2])

    def test_check_rpc_call_local_identity_beats_certificate(self):
        seen = self._check(environ={gate.UNIX_PEER_ENVIRON_KEY: {"user": "alice", "uid": 1}}, cert="cert:me")
        self.assertEqual(seen["describe"][4], "unix:alice")

    def test_check_rpc_call_login_binding(self):
        peer = {gate.UNIX_PEER_ENVIRON_KEY: {"user": "alice", "uid": 1}}
        seen = self._check(environ=dict(peer, **{gate.UNIX_LOGINS_ENVIRON_KEY: frozenset({"other"})}), login="u")
        self.assertEqual(seen["apply"][1:], (policy.DENY, "unix-login-not-bound", policy.MODE_ENFORCE))
        self.assertNotIn("decide", seen)
        seen = self._check(environ=dict(peer, **{gate.UNIX_LOGINS_ENVIRON_KEY: frozenset({"u"})}), login="u")
        self.assertEqual(seen["apply"][1:], (policy.ALLOW, "decided", policy.MODE_ENFORCE))
        seen = self._check(environ=dict(peer, **{gate.UNIX_LOGINS_ENVIRON_KEY: frozenset()}), login="u")
        self.assertEqual(seen["apply"][1], policy.ALLOW)  # empty set: any login
        # an unreadable parameter must not drop the binding
        seen = self._check(fail=True, environ=dict(peer, **{gate.UNIX_LOGINS_ENVIRON_KEY: frozenset({"other"})}), login="u")
        self.assertEqual(seen["apply"][1:3], (policy.DENY, "unix-login-not-bound"))

    # ---- wrappers ----------------------------------------------------------------------------------------------

    def test_guard_get_public_method(self):
        calls = []
        rpc = {"on": True}

        def original(model, name):
            """doc"""
            calls.append(("orig", model, name))
            return "method"
        self.safe_patch_object(guard, "call_is_rpc", new=lambda: rpc["on"])
        self.safe_patch_object(guard, "check_rpc_call", new=lambda model, name: calls.append(("check", model, name)))
        wrapped = guard.guard_get_public_method(original)
        self.assertTrue(wrapped.__dict__[guard.WRAPPED])
        self.assertEqual((wrapped.__name__, wrapped.__doc__), ("original", "doc"))
        self.assertEqual(wrapped("M", "n"), "method")
        self.assertEqual(calls, [("orig", "M", "n"), ("check", "M", "n")])
        del calls[:]
        rpc["on"] = False
        self.assertEqual(wrapped("M", "n"), "method")
        self.assertEqual(calls, [("orig", "M", "n")])

    def test_guard_get_public_method_core_refusal_comes_first(self):
        calls = []

        def original(model, name):
            raise KeyError("private")
        self.safe_patch_object(guard, "call_is_rpc", new=lambda: True)
        self.safe_patch_object(guard, "check_rpc_call", new=lambda model, name: calls.append(name))
        with self.assertRaises(KeyError):
            guard.guard_get_public_method(original)("M", "n")
        self.assertEqual(calls, [])

    def test_guard_execute_cr(self):
        seen = []

        def original(*args, **kwargs):
            """doc"""
            seen.append((args, kwargs, guard._external.get()))
            return "result"
        wrapped = guard.guard_execute_cr(original)
        self.assertTrue(wrapped.__dict__[guard.WRAPPED])
        self.assertEqual(wrapped.__name__, "original")
        self.assertFalse(guard._external.get())
        self.assertEqual(wrapped(1, 2, k=3), "result")
        self.assertEqual(seen, [((1, 2), {"k": 3}, True)])
        self.assertFalse(guard._external.get())

    def test_guard_execute_cr_resets_after_an_error(self):
        def original():
            raise KeyError("x")
        with self.assertRaises(KeyError):
            guard.guard_execute_cr(original)()
        self.assertFalse(guard._external.get())

    def test_install_wraps_once(self):
        def gpm(model, name):
            return None

        def cr(*args):
            return None
        self.safe_patch_object(service_model, "get_public_method", new=gpm)
        self.safe_patch_object(service_model, "execute_cr", new=cr)
        self.safe_patch_object(json2, "get_public_method", new=gpm)
        guard.install()
        first_gpm, first_cr = service_model.get_public_method, service_model.execute_cr
        self.assertTrue(first_gpm.__dict__[guard.WRAPPED])
        self.assertTrue(first_cr.__dict__[guard.WRAPPED])
        self.assertIs(first_gpm.__wrapped__, gpm)
        self.assertIs(first_cr.__wrapped__, cr)
        self.assertIs(json2.get_public_method, first_gpm)
        guard.install()
        self.assertIs(service_model.get_public_method, first_gpm)
        self.assertIs(service_model.execute_cr, first_cr)
        self.assertIs(json2.get_public_method, first_gpm)

    def test_install_does_not_replace_a_wrapped_json2(self):
        def gpm(model, name):
            return None
        wrapped_already = guard.guard_get_public_method(gpm)
        self.safe_patch_object(service_model, "get_public_method", new=gpm)
        self.safe_patch_object(service_model, "execute_cr", new=guard.guard_execute_cr(lambda: None))
        self.safe_patch_object(json2, "get_public_method", new=wrapped_already)
        guard.install()
        self.assertIs(json2.get_public_method, wrapped_already)
        self.assertTrue(service_model.get_public_method.__dict__[guard.WRAPPED])
