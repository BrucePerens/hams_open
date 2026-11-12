# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import json
import os
import tempfile

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.hams_rpc_guard import exposure, exposure_scan, public_rpc

SAMPLE = '''
from odoo import api, models


class Thing(models.Model):
    _name = "x.thing"

    def public_one(self):
        pass

    @api.model
    def public_two(self):
        pass

    @api.private
    def hidden(self):
        pass

    def _private(self):
        pass

    @staticmethod
    def static_one():
        pass

    @property
    def SELF_WRITEABLE_FIELDS(self):
        return []

    def write(self, vals):
        return super().write(vals)

    @public_rpc
    def declared_one(self):
        pass


class Extension(models.Model):
    _inherit = ["res.partner", "mail.thread"]

    def extra(self):
        pass


class NotAModel:
    def plain(self):
        pass
'''


@tagged("post_install", "-at_install")
class TestDecorator(HamsTransactionCase):
    """The `@public_rpc` decorator and the lookup of its marks."""
    # Tests [@ANCHOR: hams_rpc_guard:public_rpc]
    # Tests [@ANCHOR: hams_rpc_guard:decorator_marks]

    def test_marks_are_found_through_the_mro(self):
        class Base:
            @public_rpc
            def plain(self):
                pass

            @public_rpc(portal=True, clients=("daemon_a",))
            def flagged(self):
                pass

        class Child(Base):
            def plain(self):  # an override without the mark: the parent's mark still counts
                pass

        self.assertEqual(exposure.decorator_marks(Child, "plain"), [{"portal": False, "clients": ()}])
        self.assertEqual(exposure.decorator_marks(Child, "flagged"), [{"portal": True, "clients": ("daemon_a",)}])
        self.assertEqual(exposure.decorator_marks(Child, "missing"), [])

    def test_the_decorator_returns_the_function_itself(self):
        def function():
            return 7
        self.assertIs(public_rpc(function), function)
        self.assertEqual(function(), 7)


@tagged("post_install", "-at_install")
class TestExposureFiles(HamsTransactionCase):
    """Reading `rpc_exposure.json` documents and the per-registry merge."""
    # Tests [@ANCHOR: hams_rpc_guard:exposure_init]
    # Tests [@ANCHOR: hams_rpc_guard:exposure_merge]
    # Tests [@ANCHOR: hams_rpc_guard:pairs]
    # Tests [@ANCHOR: hams_rpc_guard:load_exposure]
    # Tests [@ANCHOR: hams_rpc_guard:manifest_of]

    def test_merge_reads_every_section_and_ignores_junk(self):
        result = exposure.Exposure()
        with self.assertLogs(exposure._logger, "WARNING"):
            result.merge({
                "internal": {"a.b": ["m1", "m2"], "bad": "notalist"},
                "portal": {"a.b": ["m1"]},
                "stock": {"res.partner": ["read"]},
                "clients": {"daemon": {"sources": ["x/daemon/main.py"], "allow": {"a.b": ["m3"]}}, "short": {"c.d": ["m4"]}},
                "mystery": 1,
            }, "origin")
        self.assertEqual(result.internal, {("a.b", "m1"), ("a.b", "m2")})
        self.assertEqual(result.portal, {("a.b", "m1")})
        self.assertEqual(result.stock, {("res.partner", "read")})
        self.assertEqual(result.clients, {"daemon": {("a.b", "m3")}, "short": {("c.d", "m4")}})
        self.assertEqual(result.sources, {"daemon": ["x/daemon/main.py"]})

    def test_a_clients_section_that_is_not_a_mapping_is_ignored_not_fatal(self):
        for junk in (["daemon"], "daemon", 7):
            with self.subTest(junk=junk):
                result = exposure.Exposure()
                with self.assertLogs(exposure._logger, "WARNING"):
                    result.merge({"internal": {"a.b": ["m1"]}, "clients": junk}, "origin")
                self.assertEqual(result.internal, {("a.b", "m1")})
                self.assertEqual(result.clients, {})
        result = exposure.Exposure()
        result.merge({"clients": {"daemon": {"allow": {"a.b": ["m3"]}, "sources": 5}}}, "origin")
        self.assertEqual(result.clients, {"daemon": {("a.b", "m3")}})
        self.assertEqual(result.sources, {})

    def test_the_installed_addons_files_are_merged_once_per_registry(self):
        loaded = exposure.load_exposure(self.env.cr, self.env.registry)
        self.assertIn("hams_rpc_guard", loaded.modules)
        self.assertIn(("ham.dx.spot", "get_recent_spots"), loaded.portal, "the portal baseline shipped with this module")
        self.assertIs(exposure.load_exposure(self.env.cr, self.env.registry), loaded, "cached")

    def test_the_manifest_of_a_module(self):
        self.assertEqual(exposure.manifest_of("hams_rpc_guard").get("author"), "Bruce Perens K6BP")
        self.assertIsNone(exposure.manifest_of("no_such_addon_exists"))


@tagged("post_install", "-at_install")
class TestScanner(HamsTransactionCase):
    """The source scanner that generates the transitional allowlist."""
    # Tests [@ANCHOR: hams_rpc_guard:scan_orm_names]
    # Tests [@ANCHOR: hams_rpc_guard:scan_model_of_class]
    # Tests [@ANCHOR: hams_rpc_guard:scan_source]
    # Tests [@ANCHOR: hams_rpc_guard:scan_addon]
    # Tests [@ANCHOR: hams_rpc_guard:scan_render]
    # Tests [@ANCHOR: hams_rpc_guard:scan_managed_addons]
    # Tests [@ANCHOR: hams_rpc_guard:scan_daemon_pairs]
    # Tests [@ANCHOR: hams_rpc_guard:scan_main]
    # Tests [@ANCHOR: hams_rpc_guard:scan_decorator_names]
    # Tests [@ANCHOR: hams_rpc_guard:scan_string_values]

    def test_public_methods_only(self):
        found = exposure_scan.scan_source(SAMPLE, exposure_scan.orm_names())
        self.assertEqual(sorted(found["x.thing"]["public"]), ["declared_one", "public_one", "public_two"])
        self.assertEqual(found["x.thing"]["marked"], {"declared_one"})
        self.assertEqual(found["res.partner"]["public"], {"extra"}, "the first _inherit is the model")
        self.assertNotIn("write", found["x.thing"]["public"])
        self.assertNotIn("SELF_WRITEABLE_FIELDS", found["x.thing"]["public"], "a property is not a callable method")
        self.assertNotIn("NotAModel", json.dumps(sorted(found)))

    def test_an_addon_is_scanned_and_rendered_and_the_check_notices_staleness(self):
        with tempfile.TemporaryDirectory() as root:
            addon = os.path.join(root, "demo")
            os.makedirs(os.path.join(addon, "models"))
            os.makedirs(os.path.join(addon, "tests"))
            with open(os.path.join(addon, "__manifest__.py"), "w") as handle:
                handle.write('{"name": "Demo", "author": "x"}')
            with open(os.path.join(addon, "models", "thing.py"), "w") as handle:
                handle.write(SAMPLE)
            with open(os.path.join(addon, "tests", "test_thing.py"), "w") as handle:
                handle.write(SAMPLE.replace("x.thing", "x.in_tests"))
            self.assertEqual(exposure_scan.managed_addons(root), ["demo"])
            scanned = exposure_scan.scan_addon(addon)
            self.assertEqual(scanned, {"res.partner": ["extra"], "x.thing": ["public_one", "public_two"]})
            self.assertEqual(exposure_scan.main(["--check", root]), 1)
            self.assertEqual(exposure_scan.main(["--write", root]), 0)
            self.assertEqual(exposure_scan.main(["--check", root]), 0)
            with open(os.path.join(addon, "rpc_exposure.json")) as handle:
                document = json.load(handle)
            self.assertEqual(document["internal"], scanned)
            # Other sections survive a regeneration.
            document["clients"] = {"d": {"allow": {"a.b": ["c"]}}}
            rendered = exposure_scan.render(scanned, document)
            self.assertEqual(json.loads(rendered)["clients"], document["clients"])

    def test_daemon_pairs_from_calls_and_urls(self):
        source = (
            'client.execute("pager.incident", "report_incident", vals=1)\n'
            'self._json2_call("backup.job", "write", ids=[1])\n'
            'url = f"{base}/json/2/hams_helpdesk.exception.fingerprint/report_daemon_failure"\n'
            'client.execute("not a model", "x")\n'
            'other("pager.incident", "ignored_not_a_caller")\n')
        self.assertEqual(exposure_scan.daemon_pairs(source), {
            ("pager.incident", "report_incident"), ("backup.job", "write"),
            ("hams_helpdesk.exception.fingerprint", "report_daemon_failure")})
