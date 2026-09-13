# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0


from . import common
from odoo.exceptions import AccessError, UserError
from odoo import _

import os
import sys
from odoo.tests.common import tagged
from odoo.tools import file_open

@tagged("post_install", "-at_install")
class TestZeroSudoFixes(common.HamsTransactionCase):
    def test_short_circuit_res_users(self):
        # Tests [@ANCHOR: zero_sudo:hams_transaction_case_setup]

        # Tests [@ANCHOR: zero_sudo:hams_transaction_case_teardown_class]

        # Tests [@ANCHOR: zero_sudo:patched_basecase_teardown]
        # (setUp() runs before this and every other test in this class;
        # tearDownClass() runs once after the whole class finishes;
        # _patched_basecase_teardown replaces BaseCase.tearDown globally,
        # so it fires on every single test's teardown in the whole
        # process, this one included)
        regular = self.env["res.users"].create({
            "name": "Regular User",
            "login": "regular_user_test@example.com",
            "lang": "en_US",
        })
        service = self.env["res.users"].create({
            "name": "Service Account",
            "login": "service_account_test@example.com",
            "is_service_account": True,
            "lang": "en_US",
        })
        
        users = regular | service
        users.write({"name": "Updated Name"})
        
        self.assertEqual(regular.name, "Updated Name")
        self.assertEqual(service.name, "Updated Name")

    def test_write_splits_a_mixed_batch_so_service_accounts_never_get_the_given_password(self):
        # Tests [@ANCHOR: zero_sudo:res_users_write]
        # write()'s own real purpose (unlike test_short_circuit_res_users
        # above, which only exercises its trivial neither-branch fall-
        # through with a plain name write): a batch write that sets
        # "password" across a MIXED recordset must never let that
        # password reach a service account -- only the regular accounts
        # in the batch get it, service accounts keep getting their own
        # forced-random one.
        regular = self.env["res.users"].create(
            {"name": "Regular Batch User", "login": "regular_batch_test@example.com", "lang": "en_US"}
        )
        service = self.env["res.users"].create(
            {"name": "Service Batch User", "login": "service_batch_test@example.com", "is_service_account": True, "lang": "en_US"}
        )
        (regular | service).write({"password": "attacker_supplied_password"})
        self.env.cr.flush()

        credential = {
            "login": "regular_batch_test@example.com",
            "password": "attacker_supplied_password",
            "type": "password",
        }
        auth_info = self.env["res.users"].authenticate(credential, {"interactive": False})
        self.assertEqual(auth_info["uid"], regular.id)

        with self.assertRaises(Exception):
            bad_credential = dict(credential, login="service_batch_test@example.com")
            self.env["res.users"].authenticate(bad_credential, {"interactive": False})

    def test_write_de_designating_a_service_account_still_forces_a_random_password(self):
        # Tests [@ANCHOR: zero_sudo:res_users_write]

        # Real bug: is_service_account=True forced a random password (the
        # `if vals.get("is_service_account")` branch), but is_service_account
        # =False, supplied in the SAME write() call as a real "password",
        # matched neither that branch (falsy value) nor the elif branch
        # (which requires "is_service_account" not in vals at all) -- it fell
        # through to a plain super().write(vals) that applied the
        # caller-chosen password as-is, letting a caller silently strip
        # service-account status AND plant a known, loginable credential in
        # one atomic call.
        service = self.env["res.users"].create(
            {
                "name": "Formerly Service Account",
                "login": "formerly_service_test@example.com",
                "is_service_account": True,
                "lang": "en_US",
            }
        )
        service.write(
            {"is_service_account": False, "password": "attacker_supplied_password"}
        )
        self.env.cr.flush()

        self.assertFalse(service.is_service_account)

        with self.assertRaises(Exception):
            bad_credential = {
                "login": "formerly_service_test@example.com",
                "password": "attacker_supplied_password",
                "type": "password",
            }
            self.env["res.users"].authenticate(bad_credential, {"interactive": False})

    def test_write_invalidates_the_is_service_account_cache(self):
        # Tests [@ANCHOR: zero_sudo:res_users_write]

        # Bug-hunt fix, 2026-09-13: `ir.http._is_service_account_cached` is
        # `@distributed_cache()`-decorated -- an L1, process-lifetime cache
        # checked BEFORE Redis's own 24h TTL is ever consulted. Before this
        # fix, write() never invalidated it, so a worker that had already
        # cached a uid's OLD is_service_account value kept serving that
        # stale verdict indefinitely, not just for up to 24h. This is the
        # exact incident-response scenario the flag exists for: promoting a
        # compromised user to a service account (to block their interactive
        # Web UI access, per ir_http._authenticate's own gate) must actually
        # take effect, not silently keep honoring the pre-promotion cached
        # answer.
        #
        # Asserting `notify_model_invalidation` was actually invoked, rather
        # than trying to observe the real end-to-end cache-clearing effect,
        # matches this test class's own established convention (see e.g.
        # `test_15_invalidate_model_cache` in security_utils.py's own test
        # suite) and this file's own `safe_patch_object`'s documented reason
        # for it: proving the real postcommit-deferred side effect directly
        # needs a genuine `env.cr.commit()`, which
        # `common.HamsTransactionCase` (this class) explicitly forbids
        # (`check_burn_list.py`'s own TEST CURSOR CORRUPTION rule) precisely
        # because it corrupts the shared test cursor other tests in this
        # same run still need -- `RealTransactionCase` exists for tests that
        # genuinely need that, and this one doesn't: the actual clearing
        # behavior of `notify_model_invalidation`/`invalidate_model_cache`
        # is already covered by security_utils.py's own tests.
        user = self.env["res.users"].create(
            {
                "name": "Soon To Be Service Account",
                "login": "soon_service_account_test@example.com",
                "is_service_account": False,
                "lang": "en_US",
            }
        )

        mock_notify = self.safe_patch(
            "odoo.addons.zero_sudo.models.res_users.notify_model_invalidation"
        )
        user.write({"name": "Renamed, Not Yet A Service Account"})
        mock_notify.assert_not_called()

        user.write({"is_service_account": True})
        mock_notify.assert_called_once_with(self.env, "ir.http")

    def test_get_callsign_generates_unique_cached_synthetic_callsigns(self):
        # Tests [@ANCHOR: zero_sudo:get_callsign]

        # Tests [@ANCHOR: zero_sudo:generate_test_callsign]
        # get_callsign()/generate_test_callsign() are used constantly by
        # hams_com's own ham-radio test suites (get_callsign() dozens of
        # times per module) but never directly by anything in hams_open
        # itself -- a real, direct test of their own mechanism, not just
        # trusting cross-repo usage this repo's own verify_anchors.py
        # can't see anyway.
        first = self.get_callsign("test_key_a")
        second = self.get_callsign("test_key_b")
        self.assertNotEqual(first, second, "Different keys must get different callsigns.")
        self.assertEqual(
            self.get_callsign("test_key_a"),
            first,
            "The same key must return the SAME cached callsign on a repeat call.",
        )
        self.assertRegex(first, r"^T\d{4}X$")

    def test_ir_module_module_access_error(self):
        # Tests [@ANCHOR: zero_sudo:bootstrap_knowledge_docs]

        # Tests [@ANCHOR: zero_sudo:ir_module_register_hook]
        # _register_hook() itself is trivially proven by every real module
        # install this whole test process performs (it's a core Odoo
        # registry-build lifecycle hook, always run) -- if it crashed, no
        # test in this run would ever start at all. This test is the
        # closest direct exercise of the method it calls.
        utils = self.env["zero_sudo.security.utils"]
        
        def mock_get_service_uid(self_inst, xmlid, raise_if_not_found=True):
            if xmlid == "zero_sudo.odoo_facility_service_internal":
                raise AccessError(_("Simulated Access Error"))
            return 1
            
        self.safe_patch_object(type(utils), '_get_service_uid', mock_get_service_uid)
            
        # This should not raise an exception
        self.env["ir.module.module"]._bootstrap_knowledge_docs()

    def test_bootstrap_knowledge_docs_default_unpublished(self):
        # Tests [@ANCHOR: zero_sudo:install_single_doc]
        # Regression test: module-authored knowledge_docs (architecture notes,
        # security internals, runbooks, developer/story/journey guides) must
        # default to is_published=False. A prior bug forced every ingested doc
        # to is_published=True unconditionally, leaking internal engineering
        # documentation (e.g. "Zero-Sudo Security Core") into the public
        # website's anonymous help sidebar. See night_shift_todo.md.
        Module = self.env["ir.module.module"]
        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]

        doc_info = {
            "name": "Test Internal Doc Regression",
            "path": "data/documentation.html",
        }
        Module._install_single_doc(utils, Article, "zero_sudo", doc_info)
        article = Article.search([("name", "=", "Test Internal Doc Regression")], limit=1)
        self.assertTrue(article, "Doc should have been created")
        self.assertFalse(
            article.is_published,
            "Module-authored docs must default to unpublished (internal-only) "
            "unless the manifest entry explicitly sets \"public\": True",
        )

    def test_bootstrap_knowledge_docs_explicit_public_opt_in(self):
        Module = self.env["ir.module.module"]
        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]

        doc_info = {
            "name": "Test Public Doc Regression",
            "path": "data/documentation.html",
            "public": True,
        }
        Module._install_single_doc(utils, Article, "zero_sudo", doc_info)
        article = Article.search([("name", "=", "Test Public Doc Regression")], limit=1)
        self.assertTrue(article, "Doc should have been created")
        self.assertTrue(
            article.is_published,
            "A knowledge_docs entry with \"public\": True should be published",
        )

    def test_daemon_utils_sys_paths(self):
        daemon_utils = self.env["zero_sudo.daemon.utils"]
        
        captured_env = {}
        def mock_popen(*args, **kwargs):
            captured_env.update(kwargs.get("env", {}))
            class MockProcess:
                pid = 1234
            return MockProcess()
            
        self.safe_patch("subprocess.Popen", mock_popen)
        
        daemon_utils._start_daemon_process("/dev/null")
        pythonpath = captured_env.get("PYTHONPATH", "")
        if sys.path[0]:
            self.assertIn(sys.path[0], pythonpath)
        self.assertNotEqual(pythonpath, "/usr/lib/python3/dist-packages")

    def test_ir_http_is_service_account_cached(self):
        # [@ANCHOR: zero_sudo:COMM_test_is_service_account_cached]
        user = self.env["res.users"].create({
            "name": "Service Account IrHttp",
            "login": "service_account_irhttp@example.com",
            "is_service_account": True,
            "lang": "en_US",
        })
        self.assertTrue(self.env["ir.http"]._is_service_account_cached(user.id))
        
        user2 = self.env["res.users"].create({
            "name": "Regular IrHttp",
            "login": "regular_irhttp@example.com",
            "is_service_account": False,
            "lang": "en_US",
        })
        self.assertFalse(self.env["ir.http"]._is_service_account_cached(user2.id))

    def test_res_users_filtered(self):
        # Test for models/res_users.py:45
        user = self.env["res.users"].create({
            "name": "Filtered User",
            "login": "filtered_user@example.com",
            "is_service_account": True,
            "lang": "en_US",
        })
        user.write({"password": "new_password"})
        self.assertNotEqual(user.password, "new_password")

    def test_security_log_autovacuum(self):
        # [@ANCHOR: zero_sudo:COMM_test_security_log_autovacuum]

        # Tests [@ANCHOR: zero_sudo:security_log_autovacuum]
        log = self.env["zero_sudo.security.log"].create({
            "reason": "cache_invalidation"
        })
        self.env["zero_sudo.security.log"].autovacuum()
        self.assertTrue(log.exists())

        cron = self.env.ref("zero_sudo.ir_cron_security_log_autovacuum")
        service_user = self.env.ref("zero_sudo.odoo_facility_service_internal")
        self.assertEqual(cron.user_id, service_user, "Cron must run as zero_sudo.odoo_facility_service_internal")

    def test_security_log_autovacuum_cron_actually_runs_as_its_service_user(self):
        # Tests [@ANCHOR: zero_sudo:COMM_security_log_autovacuum_cron_runs]
        # test_security_log_autovacuum above calls autovacuum() directly as
        # this test's own (effectively superuser) env, and only checks
        # cron.user_id -- it never actually runs the cron's underlying
        # ir.actions.server through the real execution path the scheduler
        # uses. That path calls _can_execute_action_on_records(), which
        # requires WRITE access to the cron's declared model_id (zero_sudo.
        # security.log) before it will run the action at all -- a real,
        # reproducible failure found live: ir.model.access.csv deliberately
        # keeps this model read/create-only for the facility service (an
        # audit log shouldn't be ORM-writable, even by its own cleanup job),
        # so the cron 500'd/failed with "Forbidden server action" on every
        # scheduled run until group_ids was set on the action itself to
        # authorize it without broadening the model's actual ACL.
        cron = self.env.ref("zero_sudo.ir_cron_security_log_autovacuum")
        cron.ir_actions_server_id.with_user(cron.user_id.id).run()

    def test_poll_health_check(self):
        # [@ANCHOR: zero_sudo:COMM_test_poll_health_check]
        daemon_utils = self.env["zero_sudo.daemon.utils"]
        
        class MockResponse:
            status = 200
            def __enter__(self): return self
            def __exit__(self, *args): pass
            
        def mock_urlopen(*args, **kwargs):
            return MockResponse()
            
        self.safe_patch("urllib.request.urlopen", mock_urlopen)
        host = os.environ.get("DAEMON_HOST", "odoo")
        res = daemon_utils._poll_health_check(f"http://{host}:8080/health", timeout=1, interval=0.1)
        self.assertTrue(res)

    def test_daemon_utils_rpc_security(self):
        # [@ANCHOR: zero_sudo:COMM_test_daemon_utils_rpc_security]
        daemon_utils = self.env["zero_sudo.daemon.utils"]
        with self.assertRaises(AttributeError, msg="start_daemon_process must be private to prevent RCE via RPC"):
            daemon_utils.start_daemon_process("/dev/null")
        with self.assertRaises(AttributeError, msg="stop_daemon_process must be private"):
            daemon_utils.stop_daemon_process(None)
        with self.assertRaises(AttributeError, msg="poll_health_check must be private to prevent SSRF via RPC"):
            daemon_utils.poll_health_check("http://odoo")

    def test_security_log_immutability(self):
        # [@ANCHOR: zero_sudo:COMM_test_security_log_immutability]
        log = self.env["zero_sudo.security.log"].create({
            "reason": "param_access_denied"
        })
        # Check system group
        system_user = self.env.ref("base.user_admin")
        log_sudo = log.with_user(system_user)
        with self.assertRaises(AccessError):
            log_sudo.write({"reason": "changed"})
            self.env.flush_all()
        with self.assertRaises(AccessError):
            log_sudo.unlink()
            self.env.flush_all()
            
        # Check facility service group
        facility_user = self.env.ref("zero_sudo.odoo_facility_service_internal")
        log_facility = log.with_user(facility_user)
        with self.assertRaises(AccessError):
            log_facility.write({"reason": "changed_facility"})
            self.env.flush_all()
        with self.assertRaises(AccessError):
            log_facility.unlink()
            self.env.flush_all()

    def test_documentation_wrappers(self):
        # [@ANCHOR: zero_sudo:COMM_test_documentation_wrappers]

        # file_open() forces utf-8 internally for text mode already and
        # doesn't accept an encoding= kwarg of its own.
        with file_open("zero_sudo/data/testing_documentation.html", "r") as f:
            content = f.read().strip()
        self.assertTrue(content.startswith('<div class="o_knowledge_content">'))
        self.assertTrue(content.endswith('</div>'))

    def test_rpc_path_exemption_requires_exact_or_delimited_prefix(self):
        # Tests [@ANCHOR: zero_sudo:is_rpc_path_exempt_from_service_account_block]
        # Regression test for a real 2026-09-13 bug-hunt finding: the old
        # bare .startswith('/jsonrpc')/.startswith('/xmlrpc') check would
        # have exempted ANY path sharing that prefix, not just Odoo's own
        # two real RPC endpoints. Latent (no colliding route exists in this
        # codebase today), but this test locks in the tightened boundary so
        # a future route can't silently reopen it.
        ir_http = self.env["ir.http"]
        exempt = ir_http._is_rpc_path_exempt_from_service_account_block
        # Real endpoints: must remain exempt.
        self.assertTrue(exempt("/jsonrpc"))
        self.assertTrue(exempt("/xmlrpc/common"))
        self.assertTrue(exempt("/xmlrpc/2/object"))
        # Prefix-sharing look-alikes that are NOT the real endpoints: must
        # NOT be exempt.
        self.assertFalse(exempt("/jsonrpc_evil"))
        self.assertFalse(exempt("/jsonrpc2"))
        self.assertFalse(exempt("/xmlrpcevil"))
        self.assertFalse(exempt("/xmlrpc"))  # no trailing segment at all
        self.assertFalse(exempt("/web"))
        self.assertFalse(exempt("/odoo"))

    def test_install_single_doc_rejects_sibling_prefix_traversal(self):
        # Tests [@ANCHOR: zero_sudo:is_path_within_module_dir]
        # Regression test for a real 2026-09-13 bug-hunt finding: a bare
        # `resolved_path.startswith(base_dir)` (no separator) would have
        # treated a sibling directory that merely shares base_dir as a
        # string PREFIX (e.g. ".../addons/zero_sudo_evil" starting with
        # ".../addons/zero_sudo") as "inside" the module directory.
        ir_module = self.env["ir.module.module"]
        is_within = ir_module._is_path_within_module_dir
        base_dir = "/opt/hams/addons/zero_sudo"
        # Genuinely inside: allowed.
        self.assertTrue(is_within(base_dir, base_dir))
        self.assertTrue(is_within(base_dir, base_dir + "/data/documentation.html"))
        # A sibling directory that is a bare string-prefix match but NOT
        # actually inside base_dir: must be rejected.
        self.assertFalse(is_within(base_dir, base_dir + "_evil/secret.txt"))
        self.assertFalse(is_within(base_dir, base_dir + "_evil"))
        # Unrelated/absolute escape: must be rejected.
        self.assertFalse(is_within(base_dir, "/etc/passwd"))

    def test_poll_health_check_rejects_non_http_scheme(self):
        # Tests [@ANCHOR: zero_sudo:COMM_test_poll_health_check]
        # Regression test for a real 2026-09-13 bug-hunt finding: urlopen()
        # honors whatever scheme it's given, including file://, which would
        # let a caller building this URL from anything less trusted than a
        # hardcoded test string turn a health check into an arbitrary local
        # file read (or a request against an unintended internal scheme).
        # Already unreachable via RPC today (test_daemon_utils_rpc_security),
        # so this is defense-in-depth, but it must fail LOUDLY (UserError),
        # never silently attempt the request.
        daemon_utils = self.env["zero_sudo.daemon.utils"]

        def mock_urlopen(*args, **kwargs):
            self.fail("urlopen() must never be called for a non-http(s) scheme")

        self.safe_patch("urllib.request.urlopen", mock_urlopen)
        with self.assertRaises(UserError):
            daemon_utils._poll_health_check("file:///etc/passwd", timeout=1, interval=0.1)

    def test_stop_daemon_process_survives_toctou_exit(self):
        # Tests [@ANCHOR: zero_sudo:stop_daemon_process]
        # Regression test for a real 2026-09-13 bug-hunt finding: process.
        # poll() only proves the process was alive at that instant -- if it
        # exits between that check and os.getpgid(pid), the old code let a
        # bare ProcessLookupError propagate uncaught instead of treating
        # "already exited" as the success it actually is.
        daemon_utils = self.env["zero_sudo.daemon.utils"]

        class FakeProcess:
            """poll() lies and claims the process is still running --
            simulating the exact TOCTOU race window _stop_daemon_process
            must survive -- while os.getpgid() is patched below to raise
            exactly what a real already-exited PID would raise. Deliberately
            NOT exercised against a real PID: even a genuinely-reaped real
            process risks the kernel having already reused its PID for an
            unrelated process by the time this test's own os.getpgid() call
            runs, which could send a real signal to that unrelated process's
            group -- unacceptable on a shared dev box."""

            pid = 424242  # Only ever passed to the patched os.getpgid below.

            def poll(self):
                return None

            def wait(self, timeout=None):
                raise AssertionError(
                    "wait() should not be reached once the process is already gone"
                )

        def fake_getpgid(pid):
            raise ProcessLookupError(3, "No such process")

        self.safe_patch("os.getpgid", fake_getpgid)

        # Must not raise ProcessLookupError.
        daemon_utils._stop_daemon_process(FakeProcess())
