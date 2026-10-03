# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import ast
import importlib.util
import os
import xml.etree.ElementTree as ET

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

MODULE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MIGRATION_PATH = os.path.join(
    MODULE_DIR, "migrations", "1.1", "pre-migration.py"
)
NOUPDATE_TRUE = ("1", "True", "true")
CONTROL_XMLID = "protects_hams_page"


def _load_migration():
    # Load the real migration file, the same one Odoo's upgrade runs.
    spec = importlib.util.spec_from_file_location(
        "compliance_pre_migration_1_1", MIGRATION_PATH
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _declared_noupdate_by_xmlid():
    """Map each id declared in compliance's manifest XML data files to
    whether it sits in a noupdate block, following the same <odoo>/<data>
    noupdate nesting convert.py uses."""
    manifest_path = os.path.join(MODULE_DIR, "__manifest__.py")
    with open(manifest_path, encoding="utf-8") as fh:
        tree = ast.parse(fh.read())
    manifest = ast.literal_eval(
        next(n.value for n in tree.body if isinstance(n, ast.Expr))
    )
    declared = {}

    def walk(element, noupdate):
        for child in element:
            child_noupdate = noupdate
            is_block = child.tag in ("odoo", "data")
            if is_block and child.get("noupdate") is not None:
                child_noupdate = child.get("noupdate") in NOUPDATE_TRUE
            if child.get("id"):
                declared[child.get("id")] = child_noupdate
            walk(child, child_noupdate)

    for path in manifest.get("data", []):
        if path.endswith(".xml"):
            root = ET.parse(os.path.join(MODULE_DIR, path)).getroot()
            walk(root, root.get("noupdate") in NOUPDATE_TRUE)
    return declared


@tagged("post_install", "-at_install")
class TestNoupdateMigration(HamsTransactionCase):
    # Tests [@ANCHOR: compliance:clear_stale_template_noupdate_flags]

    def _noupdate(self, name):
        self.env.cr.execute(  # audit-ignore-sql: static SQL; name is a constant, not user input
            "SELECT noupdate FROM ir_model_data"
            " WHERE module = 'compliance' AND name = %s",
            (name,),
        )
        row = self.env.cr.fetchone()
        self.assertTrue(row, f"compliance.{name} has no ir_model_data row")
        return row[0]

    def test_01_listed_templates_are_declared_outside_noupdate_blocks(self):
        migration = _load_migration()
        declared = _declared_noupdate_by_xmlid()
        for name in migration.TEMPLATE_XMLIDS:
            self.assertIn(
                name, declared,
                f"[!] DIAGNOSTIC FOR AI: {name} is in the 1.1 pre-migration's "
                "TEMPLATE_XMLIDS but is not declared in any compliance data "
                "XML file.",
            )
            self.assertFalse(
                declared[name],
                f"[!] DIAGNOSTIC FOR AI: {name} is declared inside a noupdate "
                "block. Clearing its ir_model_data.noupdate flag would let -u "
                "overwrite a record meant to stay noupdate, and the record "
                "would still be skipped by the importer's own noupdate check.",
            )

    def test_02_migration_clears_stale_flags_and_nothing_else(self):
        migration = _load_migration()
        names = migration.TEMPLATE_XMLIDS
        # The state an already-installed database is in: the row created
        # while the template sat in a noupdate block still carries the flag
        # (hams_prod and hams_dev, 2026-10-02).
        self.env.cr.execute(  # audit-ignore-sql: static SQL; the parameter is the migration's own constant tuple
            "UPDATE ir_model_data SET noupdate = true"
            " WHERE module = 'compliance' AND name IN %s",
            (names,),
        )
        # The page record deliberately stays in the noupdate block and must
        # keep its flag.
        self.assertTrue(_declared_noupdate_by_xmlid()[CONTROL_XMLID])
        self.assertNotIn(CONTROL_XMLID, names)
        self.env.cr.execute(  # audit-ignore-sql: static SQL; the parameter is this file's own constant
            "UPDATE ir_model_data SET noupdate = true"
            " WHERE module = 'compliance' AND name = %s",
            (CONTROL_XMLID,),
        )

        migration.migrate(self.env.cr, "19.0.1.0")

        for name in names:
            self.assertFalse(
                self._noupdate(name),
                f"[!] DIAGNOSTIC FOR AI: compliance.{name} still has "
                "noupdate=true after the 1.1 pre-migration, so -u compliance "
                "would keep skipping its template changes.",
            )
        self.assertTrue(
            self._noupdate(CONTROL_XMLID),
            "[!] DIAGNOSTIC FOR AI: the 1.1 pre-migration cleared noupdate on "
            "a record it does not list.",
        )

    def test_03_migration_does_nothing_on_fresh_install(self):
        migration = _load_migration()
        name = migration.TEMPLATE_XMLIDS[0]
        self.env.cr.execute(  # audit-ignore-sql: static SQL; name is the migration's own constant
            "UPDATE ir_model_data SET noupdate = true"
            " WHERE module = 'compliance' AND name = %s",
            (name,),
        )
        migration.migrate(self.env.cr, None)
        self.assertTrue(self._noupdate(name))
