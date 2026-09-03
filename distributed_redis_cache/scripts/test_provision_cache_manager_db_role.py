#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Unit tests for provision_cache_manager_db_role.py. Plain stdlib +
psycopg2 script, no Odoo dependency -- run directly via pytest/unittest,
matching hams_shared/tools/'s established sibling test_<script>.py
convention (this script had none).

provision()'s whole point is a real security property -- the role it
creates must be able to CONNECT and nothing else, no table access of any
kind -- so these tests verify that against a real local Postgres
instance, not just that the SQL runs without error. The role is dropped
in tearDown either way, so this doesn't leave privileged test roles
behind on a real Postgres instance.

Connection details come entirely from the environment -- PGHOST, PGPORT,
PGUSER, PGPASSWORD, PGDATABASE (a database the target environment's own
Odoo test suite already maintains, safe to GRANT CONNECT on) -- with no
hardcoded network address or credential fallback, matching this
platform's own check_burn_list.py policy against both (confirmed
directly: a first version with "127.0.0.1"/"odoo" defaults was flagged
for exactly this). Run e.g.:
    PGHOST=127.0.0.1 PGPORT=5432 PGUSER=odoo PGPASSWORD=odoo \\
        PGDATABASE=hams_test python3 -m pytest test_provision_cache_manager_db_role.py
"""

import os
import stat
import sys
import tempfile
import unittest
from unittest import mock

import psycopg2
from psycopg2 import sql

import provision_cache_manager_db_role as script

PG_HOST = os.environ["PGHOST"]
PG_PORT = os.environ["PGPORT"]
PG_ADMIN_USER = os.environ["PGUSER"]
PG_ADMIN_PASSWORD = os.environ["PGPASSWORD"]
TARGET_DB = os.environ["PGDATABASE"]


def _admin_connect():
    return psycopg2.connect(
        host=PG_HOST, port=PG_PORT, user=PG_ADMIN_USER, password=PG_ADMIN_PASSWORD, dbname="postgres",
    )


class WriteEnvFileTests(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp()

    def test_writes_the_real_values_and_locks_permissions_to_0600(self):
        path = os.path.join(self.tmp_dir, "sub", "cache_manager_db.env")
        script.write_env_file(path, "cache_manager_ro", "s3cr3t", "dbhost", "5432", "odoo")

        with open(path) as f:
            content = f.read()
        self.assertIn("DB_HOST=dbhost\n", content)
        self.assertIn("DB_PORT=5432\n", content)
        self.assertIn("DB_NAME=odoo\n", content)
        self.assertIn("DB_USER=cache_manager_ro\n", content)
        self.assertIn("DB_PASS=s3cr3t\n", content)

        mode = stat.S_IMODE(os.stat(path).st_mode)
        self.assertEqual(mode, 0o600, "credentials file must not be group/world readable")


class ProvisionTests(unittest.TestCase):
    ROLE_NAME = "test_cache_manager_ro_pytest"

    def setUp(self):
        self._drop_role_if_exists()

    def tearDown(self):
        self._drop_role_if_exists()

    def _drop_role_if_exists(self):
        # A role that was actually GRANTed CONNECT (provision()'s own
        # real effect) can't be dropped directly -- Postgres refuses with
        # DependentObjectsStillExist until those privileges are revoked
        # first. DROP OWNED BY handles that, but it's per-database (the
        # GRANT CONNECT happened on TARGET_DB, not on "postgres", the
        # maintenance DB this class's other connections use) -- confirmed
        # directly, not assumed: a first version of this cleanup running
        # DROP OWNED BY against "postgres" left the TARGET_DB grant
        # behind and DROP ROLE still failed the same way. Only run any of
        # this when the role actually exists, since DROP OWNED BY has no
        # IF EXISTS form and errors on a role never created (the very
        # first run of these tests).
        conn = _admin_connect()
        conn.autocommit = True
        try:
            with conn.cursor() as cur:
                cur.execute("SELECT 1 FROM pg_roles WHERE rolname = %s", (self.ROLE_NAME,))
                role_exists = cur.fetchone() is not None
        finally:
            conn.close()

        if not role_exists:
            return

        target_conn = psycopg2.connect(
            host=PG_HOST, port=PG_PORT, user=PG_ADMIN_USER, password=PG_ADMIN_PASSWORD, dbname=TARGET_DB,
        )
        target_conn.autocommit = True
        try:
            with target_conn.cursor() as cur:
                cur.execute(
                    sql.SQL("DROP OWNED BY {}").format(sql.Identifier(self.ROLE_NAME))
                )
        finally:
            target_conn.close()

        conn = _admin_connect()
        conn.autocommit = True
        try:
            with conn.cursor() as cur:
                cur.execute(
                    sql.SQL("DROP ROLE {}").format(sql.Identifier(self.ROLE_NAME))
                )
        finally:
            conn.close()

    def _role_attrs(self):
        conn = _admin_connect()
        try:
            with conn.cursor() as cur:
                cur.execute(
                    "SELECT rolsuper, rolcreatedb, rolcreaterole, rolreplication "
                    "FROM pg_roles WHERE rolname = %s",
                    (self.ROLE_NAME,),
                )
                return cur.fetchone()
        finally:
            conn.close()

    def test_creates_a_role_with_no_elevated_privileges(self):
        script.provision(PG_HOST, PG_PORT, PG_ADMIN_USER, PG_ADMIN_PASSWORD, TARGET_DB, self.ROLE_NAME)

        rolsuper, rolcreatedb, rolcreaterole, rolreplication = self._role_attrs()
        self.assertFalse(rolsuper, "the role must not be a superuser")
        self.assertFalse(rolcreatedb, "the role must not be able to create databases")
        self.assertFalse(rolcreaterole, "the role must not be able to create other roles")
        self.assertFalse(rolreplication, "the role must not have replication privilege")

    def test_the_role_can_connect_but_has_no_table_access_at_all(self):
        # The real security property this whole script exists for: real
        # end-to-end verification, not just reading the SQL and trusting
        # it. Connects AS the newly-created role and confirms it can open
        # a session (CONNECT works) but a real query against a real DATA
        # table is rejected. Deliberately not pg_catalog.pg_tables (tried
        # first): Postgres grants public SELECT on system catalog views
        # by default regardless of any role-level GRANT, so that would
        # have passed even with a real access-control bug in provision().
        # res_users is a real Odoo table with no public access
        # (confirmed directly: has_table_privilege('public', 'res_users',
        # 'SELECT') is false in this same database) -- a real, meaningful
        # negative case.
        password = script.provision(
            PG_HOST, PG_PORT, PG_ADMIN_USER, PG_ADMIN_PASSWORD, TARGET_DB, self.ROLE_NAME
        )

        role_conn = psycopg2.connect(
            host=PG_HOST, port=PG_PORT, user=self.ROLE_NAME, password=password, dbname=TARGET_DB,
        )
        try:
            with role_conn.cursor() as cur:
                # A constant-expression query needs no table privilege at
                # all -- must succeed, confirming CONNECT really works.
                cur.execute("SELECT 1")
                self.assertEqual(cur.fetchone(), (1,))

            with self.assertRaises(psycopg2.errors.InsufficientPrivilege):
                with role_conn.cursor() as cur:
                    cur.execute("SELECT * FROM res_users LIMIT 1")
        finally:
            role_conn.close()

    def _stored_password_hash(self):
        conn = _admin_connect()
        try:
            with conn.cursor() as cur:
                cur.execute("SELECT rolpassword FROM pg_authid WHERE rolname = %s", (self.ROLE_NAME,))
                return cur.fetchone()[0]
        finally:
            conn.close()

    def test_calling_provision_again_rotates_the_password(self):
        # Checks Postgres's own stored credential (pg_authid.rolpassword)
        # rather than trying a live connection with the old password --
        # this dev environment's own pg_hba.conf trust-authenticates
        # every local connection regardless of the password supplied
        # (confirmed directly via pg_hba_file_rules(): every 127.0.0.1
        # rule is "trust"), so a live-connection check here would test
        # this environment's auth config, not provision()'s own real
        # ALTER ROLE ... PASSWORD behavior.
        first_password = script.provision(
            PG_HOST, PG_PORT, PG_ADMIN_USER, PG_ADMIN_PASSWORD, TARGET_DB, self.ROLE_NAME
        )
        first_hash = self._stored_password_hash()

        second_password = script.provision(
            PG_HOST, PG_PORT, PG_ADMIN_USER, PG_ADMIN_PASSWORD, TARGET_DB, self.ROLE_NAME
        )
        second_hash = self._stored_password_hash()

        self.assertNotEqual(first_password, second_password)
        self.assertNotEqual(
            first_hash, second_hash,
            "the role's actual stored Postgres credential must change on a second provision() call",
        )


class ParseArgsPasswordSourcingTests(unittest.TestCase):
    # Adversarial security review, 2026-09-03: --admin-password used to be
    # a plain, required CLI flag -- a superuser-capable credential visible
    # in `ps aux`/`/proc/<pid>/cmdline` for the process's lifetime, and
    # typically left behind in shell history. Real tests for the two
    # replacement sourcing paths, not just the removed flag's absence.

    _BASE_ARGV = [
        "provision_cache_manager_db_role.py",
        "--admin-host", "dbhost", "--admin-user", "postgres", "--target-db", "odoo",
    ]

    def test_reads_the_password_from_the_env_var_without_prompting(self):
        with mock.patch.object(sys, "argv", self._BASE_ARGV), \
             mock.patch.dict(os.environ, {"CACHE_MANAGER_DB_ADMIN_PASSWORD": "env-supplied-pw"}), \
             mock.patch("getpass.getpass") as mock_getpass:
            args = script.parse_args()
        mock_getpass.assert_not_called()
        self.assertEqual(args.admin_password, "env-supplied-pw")

    def test_falls_back_to_an_interactive_prompt_when_the_env_var_is_unset(self):
        env_without_the_var = {k: v for k, v in os.environ.items() if k != "CACHE_MANAGER_DB_ADMIN_PASSWORD"}
        with mock.patch.object(sys, "argv", self._BASE_ARGV), \
             mock.patch.dict(os.environ, env_without_the_var, clear=True), \
             mock.patch("getpass.getpass", return_value="typed-pw") as mock_getpass:
            args = script.parse_args()
        mock_getpass.assert_called_once()
        self.assertEqual(args.admin_password, "typed-pw")

    def test_the_password_never_appears_as_a_cli_flag_the_parser_accepts(self):
        # Real, direct proof the flag is gone, not just "we didn't wire it
        # up" -- argparse must reject it outright.
        argv_with_old_flag = self._BASE_ARGV + ["--admin-password", "leaked-in-argv"]
        with mock.patch.object(sys, "argv", argv_with_old_flag):
            with self.assertRaises(SystemExit):
                script.parse_args()


if __name__ == "__main__":
    unittest.main()
