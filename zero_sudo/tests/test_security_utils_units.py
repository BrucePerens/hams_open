# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free tests of `zero_sudo.models.security_utils` (everything except the tunnel-peer helper) against a fake
environment, added to kill the mutation survivors of the second mutation pass (2026-11-13)."""
import itertools
import logging
import os
import psycopg2.errors
import shutil
import tempfile
import types
import importlib.util
from contextlib import contextmanager
from types import SimpleNamespace
from unittest.mock import mock_open

from odoo import models
from odoo.exceptions import AccessError, UserError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.zero_sudo.models import security_utils as su

_COUNTER = itertools.count()
FACILITY = "zero_sudo.odoo_facility_service_internal"
CONFIG = "zero_sudo.config_service_internal"
READ_WHITELIST = (
    "caching.enable_sw_test_hooks",
    "distributed_redis_cache.redis_host",
    "distributed_redis_cache.redis_password",
    "distributed_redis_cache.redis_pass",
    "distributed_redis_cache.redis_port",
    "distributed_redis_cache.test_integration_active",
    "web.base.url",
    "ham_dns.base_domain",
    "ham_dns.default_a_record_ip",
    "content_security_policy.report_uri",
    "backup_management.rmq_host",
    "backup_management.rmq_user",
    "backup_management.rmq_pass",
    "rabbitmq.user",
    "rabbitmq.pass",
    "rabbitmq.host",
    "rabbitmq.port",
    "rabbitmq.vhost",
    "caching.safe_quota_mb",
    "cloudflare.last_static_mtime",
    "cloudflare.tunnel.provisioned",
    "cloudflare.trusted_ip_ranges_auto",
    "cloudflare.trusted_ip_ranges_custom",
    "cloudflare.trusted_ip_ranges_last_refreshed",
    "cloudflare.trust_non_tunnel_peers",
    "pager_duty.helpdesk_model",
    "user_websites.company_abuse_email",
    "user_websites.max_sites_per_user",
    "user_websites.max_pages_per_site",
    "user_websites.allow_custom_domains",
    "user_websites.allow_html_embed",
    "user_websites.docs_installed",
    "user_websites_seo.docs_installed",
    "user_websites.global_website_page_limit",
    "user_websites.global_blog_limit",
    "user_websites.global_blog_post_limit",
    "content_security_policy.report_url",
    "user_websites.last_digest_id",
    "ham_training_training.preface_text",
    "mail.catchall.alias",
    "mail.bounce.alias",
    "mail.catchall.domain",
    "ham_map.aprs_credential",
    "ham_map.openweathermap_credential",
    "ham_map.aishub_credential",
    "pager_duty.config_dir",
    "pager_duty.domain_api_identity",
    "hams_base.compliance_org_name",
    "hams_base.compliance_mailing_address",
    "hams_base.email_delivery_notice",
    "hams_rpc_guard.allowed_networks",
    "hams_rpc_guard.legacy_access",
    "hams_rpc_guard.trusted_proxies",
    "hams_rpc_guard.client_address_header",
    "hams_rpc_guard.rpc_transport",
    "hams_rpc_guard.unix_peers",
    "hams_rpc_guard.mode",
    "hams_rpc_guard.strict_stock",
    "hams_rpc_guard.managed_authors",
    "hams_rpc_guard.client_cert_header",
    "hams_helpdesk.ncmec_api_base_url",
    "hams_helpdesk.ncmec_api_username",
    "hams_helpdesk.ncmec_api_password",
    "hams_helpdesk.ncmec_contact_email",
    "hams_helpdesk.ncmec_contact_phone",
    "hams_helpdesk.github_webhook_secret",
    "hams_helpdesk.ncmec_fallback_report_email",
    "compliance.dmca_registration_number",
    "compliance.dmca_agent_name",
    "hams_helpdesk.triage_quarantine_verdicts",
    "hams_helpdesk.triage_notify_hold_seconds",
    "hams_helpdesk.triage_notify_on_leaving_spam",
    "hams_helpdesk.triage_stall_seconds",
)

WRITE_WHITELIST = (
    "web.base.url",
    "user_websites_seo.docs_installed",
    "cloudflare.last_static_mtime",
    "cloudflare.tunnel.provisioned",
    "cloudflare.trusted_ip_ranges_auto",
    "cloudflare.trusted_ip_ranges_last_refreshed",
    "caching.safe_quota_mb",
    "user_websites.last_digest_id",
)


class FakeCursor:
    def __init__(self, dbname="db1", fetchone=None, fetchall=()):
        self.dbname = dbname
        self.executed = []
        self._fetchone, self._fetchall = fetchone, list(fetchall)
        self.savepoints = []
        self.order = []

    def execute(self, sql, params=None):
        self.order.append("execute")
        self.executed.append((sql, params))
        if isinstance(self._fetchone, Exception):
            raise self._fetchone

    def fetchone(self):
        return self._fetchone

    def fetchall(self):
        return self._fetchall

    @contextmanager
    def savepoint(self, flush=True):
        self.order.append("savepoint")
        self.savepoints.append(flush)
        yield


class FakeLog:
    """Stands in for an env mapping `zero_sudo.security.log` and `ir.config_parameter` ..."""

    def __init__(self):
        self.created = []
        self.params = {}
        self.set_calls = []
        self.kv_rows = []
        self.searches = []
        self.invalidated = 0

    def __getitem__(self, name):
        owner = self
        if name == "zero_sudo.security.log":
            return SimpleNamespace(create=lambda vals: owner.created.append(vals))
        if name == "ir.config_parameter":
            return SimpleNamespace(
                get_param=lambda key, default=None: owner.params.get(key, default) if key not in owner.params else owner.params[key],
                set_param=lambda key, value: owner.set_calls.append((key, value)) or "set-result")
        if name == "zero_sudo.kv":
            def search(domain, limit=None):
                owner.searches.append((domain, limit))
                return owner.kv_rows[0] if owner.kv_rows else None
            return SimpleNamespace(search=search)
        raise KeyError(name)


def _utils(cursor=None, user=None, registry=None, context=None, models=None):
    util = object.__new__(su.ZeroSudoSecurityUtils)

    class Env(dict):
        def __bool__(self):
            return True  # the translation machinery asks `if env:`
    env = Env(models or {})
    env.cr = cursor or FakeCursor()
    env.user = user or SimpleNamespace(id=42, has_group=lambda group: False)
    env.registry = registry
    env.uid = 42
    env.lang = 'en_US'
    env.context = context or {}
    env.flushed = []
    env.flush_all = lambda: (env.flushed.append(True), env.cr.order.append("flush"))
    util.env = env
    return util


@tagged("post_install", "-at_install")
class TestSecurityUtilsUnits(HamsTransactionCase):

    def _stub(self, name, function):
        """Replace a method of the model class for this test; `function` takes the call's arguments without self."""
        self.safe_patch_object(su.ZeroSudoSecurityUtils, name, new=lambda _self, *args, **kwargs: function(*args, **kwargs))

    def _cache(self, content):
        self.safe_patch_object(su, "_SERVICE_UID_CACHE", new=dict(content))

    # ---- preload -----------------------------------------------------------------------------------------------

    def test_register_hook_preloads(self):
        calls = []
        self._stub("_preload_service_uid_cache", lambda: calls.append(1))
        self.safe_patch_object(models.BaseModel, "_register_hook", new=lambda self: calls.append("super"))
        _utils()._register_hook()
        self.assertEqual(calls, ["super", 1])

    def test_preload_replaces_only_this_databases_entries(self):
        self._cache({("db1", "m.gone"): 1, ("db2", "m.other"): 2, ("db1", "m.kept"): 3})
        cursor = FakeCursor(dbname="db1", fetchall=[("m.kept", 30), ("m.new", 4)])
        with self.assertLogs(su._logger, level=logging.INFO) as logs:
            _utils(cursor)._preload_service_uid_cache()
        self.assertEqual(su._SERVICE_UID_CACHE, {("db2", "m.other"): 2, ("db1", "m.kept"): 30, ("db1", "m.new"): 4})
        self.assertEqual(cursor.executed, [(
            "SELECT d.module || '.' || d.name, d.res_id "
            "FROM ir_model_data d "
            "JOIN res_users u ON u.id = d.res_id "
            "WHERE d.model = 'res.users' AND u.is_service_account = true", None)])
        self.assertEqual(logs.output, [
            "INFO:%s:zero_sudo: preloaded 3 service-account UID(s) for database 'db1' into the in-process hash table." % su._logger.name])

    # ---- hash --------------------------------------------------------------------------------------------------

    def test_deterministic_hash(self):
        util = _utils()
        self.assertEqual(util._get_deterministic_hash("a"), 1251442963)
        self.assertEqual(util._get_deterministic_hash("hello world"), 961357754)
        self.assertEqual(util._get_deterministic_hash(""), 1672528963)
        self.assertEqual(util._get_deterministic_hash("é"), 1251562878)  # utf-8, not latin-1
        self.assertEqual(util._get_deterministic_hash(5), util._get_deterministic_hash("5"))
        self.assertEqual(util._get_deterministic_hash(5), 1865224830)

    # ---- _get_service_uid --------------------------------------------------------------------------------------

    def test_service_uid_refuses_malformed_ids(self):
        for bad in (None, "", 5, "nodot", b"a.b", ["a.b"]):
            with self.assertRaises(AccessError) as caught:
                _utils()._get_service_uid(bad)
            self.assertEqual(caught.exception.args[0], "Invalid XML ID format: %s. Expected 'module.name'." % (bad,))

    def test_service_uid_miss_resolves_then_returns_the_row(self):
        self._cache({("db2", "m.x"): 99})
        cursor = FakeCursor(dbname="db1", fetchone=(7,))
        util = _utils(cursor)
        self.assertEqual(util._get_service_uid("m.x"), 7)
        self.assertEqual(cursor.executed, [("SELECT zero_sudo_get_service_uid(%s)", ("m.x",))])
        self.assertEqual(cursor.savepoints, [False])
        self.assertEqual(cursor.order, ["flush", "savepoint", "execute"])
        self.assertNotIn(("db1", "m.x"), su._SERVICE_UID_CACHE)  # a miss is never written back

    def test_service_uid_hit_verifies_live_and_returns_the_cached_uid(self):
        self._cache({("db1", "m.x"): 11})
        cursor = FakeCursor(dbname="db1", fetchone=(7,))
        self.assertEqual(_utils(cursor)._get_service_uid("m.x"), 11)
        self.assertEqual(cursor.executed, [("SELECT zero_sudo_verify_service_uid(%s, %s)", (11, "m.x"))])
        self.assertEqual(cursor.savepoints, [False])

    def test_service_uid_hit_with_a_falsy_uid_is_still_a_hit(self):
        self._cache({("db1", "m.x"): 0})
        cursor = FakeCursor(dbname="db1", fetchone=(7,))
        self.assertEqual(_utils(cursor)._get_service_uid("m.x"), 0)
        self.assertEqual(cursor.executed[0][0], "SELECT zero_sudo_verify_service_uid(%s, %s)")

    def test_service_uid_translates_a_database_refusal(self):
        boom = psycopg2.errors.RaiseException("no such account")
        cursor = FakeCursor(fetchone=boom)
        with self.assertRaises(AccessError) as caught:
            _utils(cursor)._get_service_uid("m.x")
        self.assertEqual(caught.exception.args[0], "Service account resolution failed for 'm.x': no such account")
        self.assertIs(caught.exception.__cause__, boom)

    # ---- _get_service_env --------------------------------------------------------------------------------------

    def _service_env(self, context=None, base_context=None):
        log = []

        class Env:
            def __init__(self, context, company):
                self.context, self.user = context, SimpleNamespace(company_id=company)

            def __call__(self, context=None):
                log.append(("call", dict(context)))
                return Env(context, "AFTER")

        class Rec:
            def __init__(self, env):
                self.env = env

            def with_env(self, env):
                log.append(("with_env", dict(env.context)))
                return Rec(env)

            def with_company(self, company):
                log.append(("company", company))
                return Rec(Env(dict(self.env.context, company=company), company))
        util = _utils()
        asked = []
        self._stub("_get_service_uid", lambda xml_id: asked.append(xml_id) or 77)
        self._stub("with_user", lambda uid: log.append(("user", uid)) or Rec(Env(dict(base_context or {}), "BASE")))
        env = util._get_service_env("m.svc", context)
        return env, log, asked

    def test_service_env_resets_company_scope_and_sets_notrack(self):
        env, log, asked = self._service_env(base_context={"allowed_company_ids": [3, 4], "lang": "fr", "tz": "UTC"})
        self.assertEqual(asked, ["m.svc"])
        self.assertEqual(log[0], ("user", 77))
        self.assertEqual(log[1], ("call", {"lang": "fr", "tz": "UTC"}))  # the inherited company list is dropped
        self.assertEqual(log[2], ("with_env", {"lang": "fr", "tz": "UTC"}))
        self.assertEqual(log[3], ("company", "AFTER"))  # the impersonated account's own company
        self.assertEqual(env.context, {"lang": "fr", "tz": "UTC", "company": "AFTER", "mail_notrack": True})
        self.assertEqual(env.user.company_id, "AFTER")

    def test_service_env_extra_context_overrides(self):
        env, log, asked = self._service_env(context={"lang": "de", "mail_notrack": False, "extra": 1}, base_context={"lang": "fr"})
        self.assertEqual(env.context, {"lang": "de", "company": "AFTER", "mail_notrack": False, "extra": 1})
        env, log, asked = self._service_env(context={}, base_context={"lang": "fr"})
        self.assertTrue(env.context["mail_notrack"])

    # ---- ground truth / erase ----------------------------------------------------------------------------------

    def test_ground_truth_ids_bypass_access_for_exactly_the_domain(self):
        seen = []

        class Result:
            def get_result_ids(self):
                return [3, 1, 3]

        class Model:
            def _search(self, domain, **kwargs):
                seen.append((domain, kwargs))
                return Result()
        util = _utils(models={"x.y": Model()})
        self.assertEqual(util._ground_truth_ids("x.y", [("a", "=", 1)]), {1, 3})
        self.assertEqual(seen, [([("a", "=", 1)], {"bypass_access": True})])

    def _erase(self, scoped_ids, real_ids, login_row=("svc_login",)):
        unlinked = []

        class Scoped:
            ids = list(scoped_ids)

            def __bool__(self):
                return bool(self.ids)

            def unlink(self):
                unlinked.append(1)
        searches = []

        class Model:
            def with_user(self, uid):
                searches.append(("user", uid))
                return SimpleNamespace(search=lambda domain: searches.append(("search", domain)) or Scoped())
        cursor = FakeCursor(fetchone=login_row)
        util = _utils(cursor, models={"x.y": Model()})
        self._stub("_get_service_uid", lambda xml_id: searches.append(("uid", xml_id)) or 9)
        self._stub("_ground_truth_ids", lambda model, domain: searches.append(("truth", model, domain)) or set(real_ids))
        return util, cursor, unlinked, searches

    def test_erase_matching_set_unlinks_and_returns_sorted_ids(self):
        util, cursor, unlinked, searches = self._erase([5, 2, 9], [9, 5, 2])
        self.assertEqual(util._erase_via_service_account("x.y", [("d", "=", 1)], "m.svc"), [2, 5, 9])
        self.assertEqual(unlinked, [1])
        self.assertEqual(searches, [("uid", "m.svc"), ("user", 9), ("search", [("d", "=", 1)]), ("truth", "x.y", [("d", "=", 1)])])
        self.assertEqual(cursor.executed, [])

    def test_erase_with_nothing_to_delete_does_not_unlink(self):
        util, cursor, unlinked, searches = self._erase([], [])
        self.assertEqual(util._erase_via_service_account("x.y", [], "m.svc"), [])
        self.assertEqual(unlinked, [])

    def test_erase_refuses_a_partial_view_and_names_what_is_missing(self):
        util, cursor, unlinked, searches = self._erase([1], [4, 1, 3])
        with self.assertRaises(AccessError) as caught:
            util._erase_via_service_account("x.y", [], "m.svc")
        message = caught.exception.args[0]
        self.assertTrue(message.startswith("Service account 'svc_login' can only see 1 of 3 matching x.y record(s) for this erasure (missing ids: [3, 4])."))
        self.assertIn("check every ir.rule scoped to any of its groups, not just ones written specifically for it.", message)
        self.assertEqual(unlinked, [])
        self.assertEqual(cursor.executed, [("SELECT login FROM res_users WHERE id = %s", (9,))])

    def test_erase_login_falls_back_to_the_uid_and_extra_visible_rows_also_refuse(self):
        util, cursor, unlinked, searches = self._erase([1, 2], [1], login_row=None)
        with self.assertRaises(AccessError) as caught:
            util._erase_via_service_account("x.y", [], "m.svc")
        self.assertIn("Service account '9' can only see 2 of 1 matching x.y record(s)", caught.exception.args[0])
        self.assertIn("(missing ids: [])", caught.exception.args[0])
        self.assertEqual(unlinked, [])

    # ---- _ensure_executable ------------------------------------------------------------------------------------

    def test_ensure_executable(self):
        found = {"ls": "/bin/ls"}
        self.safe_patch_object(su.shutil, "which", new=lambda name: found.get(name))
        util = _utils()
        self.assertEqual(util._ensure_executable("ls", svc_xml_id="m.svc"), "/bin/ls")
        installed = []
        self._stub("_get_service_env", lambda xml_id: {
            "binary.manifest": SimpleNamespace(ensure_executable=lambda name: installed.append((xml_id, name)) or "/opt/" + name)})
        self.assertEqual(util._ensure_executable("tool", svc_xml_id="m.svc"), "/opt/tool")
        self.assertEqual(installed, [("m.svc", "tool")])
        with self.assertRaises(UserError) as caught:
            util._ensure_executable("tool")
        self.assertEqual(caught.exception.args[0], "Missing dependency: 'tool'. Please install via OS package manager (e.g., 'apt-get install tool').")
        with self.assertRaises(UserError) as caught:
            util._ensure_executable("tool", pkg_name="toolkit")
        self.assertIn("apt-get install toolkit", caught.exception.args[0])
        self.assertIn("'tool'", caught.exception.args[0])

    # ---- _resolve_dependency_cycle -----------------------------------------------------------------------------

    def _cycle(self, caller, manifest, installed, **kwargs):
        util = _utils(registry=SimpleNamespace(_init_modules={"dep"} if installed else set()))
        self._stub('_caller_module_name', lambda: caller)
        asked = []
        self.safe_patch_object(su, "odoo_get_manifest", new=lambda module: asked.append(module) or manifest)
        return util, asked

    def test_resolve_dependency_cycle(self):
        util, asked = self._cycle("callmod", {"depends_cycle": ["dep"]}, True)
        self.assertIs(util._resolve_dependency_cycle("dep"), True)
        self.assertEqual(asked, ["callmod"])
        util, asked = self._cycle("callmod", {"depends_cycle": ["dep"]}, False)
        self.assertIs(util._resolve_dependency_cycle("dep"), False)
        self.assertIs(util._resolve_dependency_cycle("dep", required=False), False)
        with self.assertRaises(UserError) as caught:
            util._resolve_dependency_cycle("dep", required=True)
        self.assertEqual(caught.exception.args[0], "This feature requires the 'dep' module, which is not installed.")
        util, asked = self._cycle("callmod", {"depends_cycle": ["dep"]}, True)
        self.assertIs(util._resolve_dependency_cycle("dep", required=True), True)

    def test_resolve_dependency_cycle_refuses_undeclared_and_unknown_callers(self):
        for manifest in ({"depends_cycle": ["other"]}, {}, {"depends_cycle": None}, {"depends": ["dep"]}):
            util, asked = self._cycle("callmod", manifest, True)
            with self.assertRaises(UserError) as caught:
                util._resolve_dependency_cycle("dep")
            self.assertEqual(caught.exception.args[0], "callmod must declare 'dep' in its own __manifest__.py 'depends_cycle' list before resolving it as a soft dependency.")
        util, asked = self._cycle(None, {"depends_cycle": ["dep"]}, True)
        with self.assertRaises(UserError) as caught:
            util._resolve_dependency_cycle("dep")
        self.assertEqual(caught.exception.args[0], "Could not determine the calling module for _resolve_dependency_cycle('dep'); refusing to proceed without verifying its 'depends_cycle' declaration.")
        self.assertEqual(asked, [])

    # ---- _caller_module_name -----------------------------------------------------------------------------------

    def test_caller_module_name_is_the_manifest_directory_of_the_calling_file(self):
        self.assertEqual(_utils()._caller_module_name(), "zero_sudo")

    def _load(self, root, relative, source):
        path = os.path.join(root, relative)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(source)
        spec = importlib.util.spec_from_file_location("callermod_%d" % next(_COUNTER), path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_caller_module_name_walks_up_to_the_nearest_manifest_and_gives_up_without_one(self):
        root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        source = "def call(util):\n    return util._caller_module_name()\n"
        with_manifest = self._load(root, "fake_addon/deep/er/caller.py", source)
        with open(os.path.join(root, "fake_addon", "__manifest__.py"), "w", encoding="utf-8") as handle:
            handle.write("{}")
        self.assertEqual(with_manifest.call(_utils()), "fake_addon")
        without = self._load(root, "no_manifest_here/sub/caller.py", source)
        self.assertIsNone(without.call(_utils()))

    def test_caller_module_name_gives_up_on_a_caller_that_is_not_a_file(self):
        root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        with open(os.path.join(root, "__manifest__.py"), "w", encoding="utf-8") as handle:
            handle.write("{}")
        here = os.getcwd()
        self.addCleanup(os.chdir, here)
        os.chdir(root)  # a relative "<not-a-file>" would otherwise resolve into a directory that has a manifest
        module_code = compile("def call(util):\n    return util._caller_module_name()\n", "<not-a-file>", "exec")
        function_code = [const for const in module_code.co_consts if isinstance(const, types.CodeType)][0]
        self.assertIsNone(types.FunctionType(function_code, {})(_utils()))

    # ---- _is_test_mode -----------------------------------------------------------------------------------------

    def test_is_test_mode(self):
        class TestCursor:
            def commit(self):
                pass

        class Plain:
            def commit(self):
                pass

        def forbidden():
            pass
        patched = Plain()
        patched.commit = forbidden
        self.assertIs(_utils(TestCursor())._is_test_mode(), True)
        self.assertIs(_utils(patched)._is_test_mode(), True)
        self.assertIs(_utils(Plain())._is_test_mode(), False)

    # ---- cache invalidation ------------------------------------------------------------------------------------

    def _invalidating(self, admin, access_error=False, known=True):
        calls = []
        log = FakeLog()

        class Model:
            def check_access(self, mode):
                calls.append(("check_access", mode))
                if access_error:
                    raise AccessError("no")

        class Env:
            pass
        asked_groups = []
        user = SimpleNamespace(id=42, has_group=lambda group: asked_groups.append(group) or admin)
        util = _utils(user=user, models={"x.y": Model()} if known else {})
        self._stub("_notify_cache_invalidation", lambda model, key: calls.append(("notify", model, key)))
        self._stub("_get_service_env", lambda xml_id: calls.append(("service_env", xml_id)) or log)
        self.safe_patch_object(su, "invalidate_model_cache", new=lambda env, model: calls.append(("invalidate", env is util.env, model)))
        return util, calls, log, asked_groups

    def test_invalidate_model_cache_for_an_admin(self):
        util, calls, log, groups = self._invalidating(admin=True)
        util._invalidate_model_cache("x.y")
        self.assertEqual(groups, ["base.group_system"])
        self.assertEqual(calls, [("invalidate", True, "x.y"), ("notify", "x.y", "CLEAR_ALL"), ("service_env", FACILITY)])
        self.assertEqual(log.created, [{"user_id": 42, "reason": "cache_invalidation", "login": "Model: x.y"}])

    def test_invalidate_model_cache_for_a_user_needs_write_access(self):
        util, calls, log, groups = self._invalidating(admin=False)
        util._invalidate_model_cache("x.y")
        self.assertEqual(calls[0], ("check_access", "write"))
        self.assertEqual(len(log.created), 1)
        util, calls, log, groups = self._invalidating(admin=False, access_error=True)
        with self.assertRaises(AccessError) as caught:
            util._invalidate_model_cache("x.y")
        self.assertEqual(caught.exception.args[0], "Security Alert: You do not have permission to invalidate the cache for model 'x.y'.")
        self.assertEqual(calls, [("check_access", "write")])
        self.assertEqual(log.created, [])

    def test_invalidate_model_cache_ignores_empty_names_and_unknown_models(self):
        for empty in (None, "", False):
            util, calls, log, groups = self._invalidating(admin=True)
            util._invalidate_model_cache(empty)
            self.assertEqual((calls, groups, log.created), ([], [], []))
        util, calls, log, groups = self._invalidating(admin=True, known=False)
        with self.assertRaises(KeyError):
            util._invalidate_model_cache("x.y")
        self.assertEqual(calls, [])

    def test_notify_cache_invalidation(self):
        seen = []
        self.safe_patch_object(su, "notify_model_invalidation", new=lambda env, model: seen.append((env, model)))
        util = _utils()
        util._notify_cache_invalidation("x.y", "CLEAR_ALL")
        self.assertEqual(seen, [(util.env, "x.y")])
        for model, key in (("", "k"), (None, "k"), ("x.y", ""), ("x.y", None), ("x.y", 0)):
            util._notify_cache_invalidation(model, key)
        self.assertEqual(len(seen), 1)

    # ---- whitelists --------------------------------------------------------------------------------------------

    def test_the_read_whitelist_is_exactly_this_list(self):
        self.assertEqual(tuple(_utils()._get_param_read_whitelist()), READ_WHITELIST)

    def test_the_write_whitelist_is_exactly_this_list(self):
        self.assertEqual(tuple(_utils()._get_param_write_whitelist()), WRITE_WHITELIST)

    # ---- _get_system_param / _set_system_param ----------------------------------------------------------------

    def _param(self):
        log = FakeLog()
        util = _utils()
        asked = []
        self._stub('_get_service_env', lambda xml_id: asked.append(xml_id) or log)
        return util, log, asked

    def test_get_system_param_returns_a_whitelisted_value_as_the_config_service(self):
        raw = su.ZeroSudoSecurityUtils._get_system_param.__wrapped__
        util, log, asked = self._param()
        key = READ_WHITELIST[0]
        log.params[key] = "value"
        self.assertEqual(raw(util, key), "value")
        self.assertEqual(raw(util, "hams_helpdesk.github_webhook_secret", "dflt"), "dflt")  # whitelisted despite 'secret'
        self.assertEqual(raw(util, READ_WHITELIST[1], default="d2"), "d2")
        self.assertEqual(asked, [CONFIG] * 3)
        self.assertEqual(log.created, [])

    def test_get_system_param_refuses_and_logs_everything_else(self):
        raw = su.ZeroSudoSecurityUtils._get_system_param.__wrapped__
        for banned in ("secret", "key", "password", "token", "auth", "crypt", "cert"):
            util, log, asked = self._param()
            key = "x.My_%s_y" % banned.upper()
            with self.assertRaises(AccessError) as caught:
                raw(util, key)
            self.assertEqual(caught.exception.args[0], "Security Alert: Parameter '%s' matches restricted cryptographic patterns and cannot be extracted via Zero-Sudo." % key)
            self.assertEqual(log.created, [{"user_id": 42, "reason": "param_access_denied", "login": key}])
            self.assertEqual(asked, [FACILITY])
        util, log, asked = self._param()
        with self.assertRaises(AccessError) as caught:
            raw(util, "x.plain")
        self.assertEqual(caught.exception.args[0], "Security Alert: Parameter 'x.plain' is not in the Zero-Sudo READ whitelist. You must explicitly register it in zero_sudo/models/security_utils.py.")
        self.assertEqual(log.created, [{"user_id": 42, "reason": "param_access_denied", "login": "x.plain"}])

    def test_set_system_param(self):
        util, log, asked = self._param()
        self.assertEqual(util._set_system_param(WRITE_WHITELIST[0], "v"), "set-result")
        self.assertEqual(log.set_calls, [(WRITE_WHITELIST[0], "v")])
        self.assertEqual(asked, [CONFIG])
        self.assertEqual(log.created, [])
        util, log, asked = self._param()
        with self.assertRaises(AccessError) as caught:
            util._set_system_param("x.plain", "v")
        self.assertEqual(caught.exception.args[0], "Security Alert: Parameter 'x.plain' is not in the Zero-Sudo WRITE whitelist. You must explicitly register it in zero_sudo/models/security_utils.py.")
        self.assertEqual(log.created, [{"user_id": 42, "reason": "param_write_denied", "login": "x.plain"}])
        self.assertEqual((log.set_calls, asked), ([], [FACILITY]))

    # ---- KV ----------------------------------------------------------------------------------------------------

    def test_get_kv(self):
        raw = su.ZeroSudoSecurityUtils._get_kv.__wrapped__
        util, log, asked = self._param()
        self.assertIsNone(raw(util, "k"))
        log.kv_rows = [SimpleNamespace(value="v")]
        self.assertEqual(raw(util, "k"), "v")
        log.kv_rows = [SimpleNamespace(value="")]
        self.assertEqual(raw(util, "k"), "")
        self.assertEqual(log.searches, [([("key", "=", "k")], 1)] * 3)
        self.assertEqual(asked, [FACILITY] * 3)

    def test_set_kv(self):
        calls = []
        cursor = FakeCursor()
        kv = SimpleNamespace(invalidate_model=lambda: calls.append("kv.invalidate_model"))
        util = _utils(cursor, models={"zero_sudo.kv": kv})
        self.safe_patch_object(su, "invalidate_model_cache", new=lambda env, model: calls.append(("cache", env is util.env, model)))
        self.safe_patch_object(su, "notify_model_invalidation", new=lambda env, model: calls.append(("notify", env is util.env, model)))
        util._set_kv("k", "v")
        self.assertEqual(cursor.executed, [("SELECT zero_sudo_set_kv(%s, %s)", ("k", "v"))])
        self.assertEqual(calls, ["kv.invalidate_model", ("cache", True, "zero_sudo.security.utils"), ("notify", True, "zero_sudo.security.utils")])

    # ---- _get_crypto_secret ------------------------------------------------------------------------------------

    def _secret(self, env=None, file_text=None, config=None, read_error=False):
        saved = os.environ.pop("HAMS_CRYPTO_KEY", None)

        def restore():
            os.environ.pop("HAMS_CRYPTO_KEY", None)
            if saved is not None:
                os.environ["HAMS_CRYPTO_KEY"] = saved
        self.addCleanup(restore)
        if env is not None:
            os.environ["HAMS_CRYPTO_KEY"] = env
        opened, checked = [], []
        self.safe_patch_object(su.os.path, "exists", new=lambda path: checked.append(path) or file_text is not None or read_error)

        def fake_open(path, mode="r"):
            opened.append((path, mode))
            if read_error:
                raise OSError("denied")
            return mock_open(read_data=file_text)()
        self.safe_patch_object(su, "open", new=fake_open, create=True)
        self.safe_patch_object(su.tools.config, "get", new=lambda key, default=None: {"admin_passwd": config}.get(key, default))
        return opened, checked

    def test_crypto_secret_environment_first(self):
        raw = su.ZeroSudoSecurityUtils._get_crypto_secret.__wrapped__
        opened, checked = self._secret(env="fromenv", file_text="fromfile", config="fromconfig")
        self.assertEqual(raw(_utils()), "fromenv")
        self.assertEqual((opened, checked), ([], []))

    def test_crypto_secret_file_second_stripped(self):
        raw = su.ZeroSudoSecurityUtils._get_crypto_secret.__wrapped__
        opened, checked = self._secret(env="", file_text="  fromfile\n", config="fromconfig")
        self.assertEqual(raw(_utils()), "fromfile")
        self.assertEqual(checked, ["/var/lib/odoo/hams_crypto.secret"])
        self.assertEqual(opened, [("/var/lib/odoo/hams_crypto.secret", "r")])

    def test_crypto_secret_config_last_and_never_the_default(self):
        raw = su.ZeroSudoSecurityUtils._get_crypto_secret.__wrapped__
        self._secret(file_text=None, config="fromconfig")
        self.assertEqual(raw(_utils()), "fromconfig")
        self._secret(file_text="   \n", config="fromconfig")
        self.assertEqual(raw(_utils()), "fromconfig")
        for config in ("admin", None, ""):
            self._secret(file_text=None, config=config)
            with self.assertLogs(su._logger, level=logging.ERROR) as logs:
                self.assertEqual(raw(_utils()), "")
            self.assertIn("No cryptographic secret is configured", logs.output[0])
        self._secret(file_text="admin", config=None)  # the default is refused from the file too
        with self.assertLogs(su._logger, level=logging.ERROR):
            self.assertEqual(raw(_utils()), "")

    def test_crypto_secret_unreadable_file_warns_and_falls_back(self):
        raw = su.ZeroSudoSecurityUtils._get_crypto_secret.__wrapped__
        self._secret(file_text=None, config="fromconfig", read_error=True)
        with self.assertLogs(su._logger, level=logging.WARNING) as logs:
            self.assertEqual(raw(_utils()), "fromconfig")
        self.assertEqual(logs.output, ["WARNING:%s:Failed to read crypto secret file: denied" % su._logger.name])
