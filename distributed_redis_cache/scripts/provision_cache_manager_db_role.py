#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
One-off operator script: provisions a dedicated, minimally-privileged
Postgres role for the cache_manager.py daemon (distributed_redis_cache),
and writes its credentials to /opt/hams/etc/keys/cache_manager_db.env.

Why this exists: cache_manager.py only ever issues `LISTEN
distributed_cache_invalidation` and a constant-expression `SELECT 1`
health check -- neither touches a table, so the daemon needs no read or
write privilege on anything. Left unconfigured, it defaults to
authenticating as the same full-privilege "odoo" role Odoo's own web
workers use across the entire schema, which is real, unnecessary
over-privilege for what this daemon actually does. This script creates a
role that can do nothing but CONNECT to the target database and LISTEN
(a Postgres pub/sub primitive that isn't gated by any table-level GRANT),
matching this codebase's zero-sudo, minimum-privilege philosophy used
everywhere else.

This does NOT write into /opt/hams/etc/keys/cache_manager.env --
daemon_key_manager's own key_registry._write_secure_env_file() truncates
and fully overwrites that file (with only ODOO_RPC_LOGIN/ODOO_RPC_KEY) on
every module install/upgrade/key rotation, which would silently wipe
DB_USER/DB_PASS back out. This writes a separate,
cache_manager.py-specific file instead (see cache_manager.py's own
DB_ENV_FILE).

Usage:
    python3 provision_cache_manager_db_role.py \\
        --admin-host <your-postgres-host> --admin-port 5432 \\
        --admin-user postgres --admin-password <admin-pw> \\
        --target-db odoo

Requires network/socket access to Postgres as a role that can CREATE ROLE
and GRANT CONNECT (e.g. the Postgres superuser) -- run this once per
deployment, then restart the cache_manager.py daemon.
"""
import argparse
import os
import secrets
import sys

import psycopg2
from psycopg2 import sql


ROLE_NAME = "cache_manager_ro"
DB_ENV_FILE = "/opt/hams/etc/keys/cache_manager_db.env"


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--admin-host", required=True, help="Postgres host/DNS name (no default -- a loopback address resolves to this script's own container, not necessarily where Postgres runs).")
    parser.add_argument("--admin-port", default="5432")
    parser.add_argument("--admin-user", required=True, help="A Postgres role that can CREATE ROLE and GRANT CONNECT (e.g. the superuser).")
    parser.add_argument("--admin-password", required=True)
    parser.add_argument("--target-db", required=True, help="The Odoo database cache_manager.py should LISTEN on (its DB_NAME).")
    parser.add_argument("--role-name", default=ROLE_NAME)
    parser.add_argument("--env-file", default=DB_ENV_FILE)
    return parser.parse_args()


def provision(admin_host, admin_port, admin_user, admin_password, target_db, role_name):
    conn = psycopg2.connect(
        host=admin_host, port=admin_port, user=admin_user, password=admin_password, dbname="postgres",
    )
    conn.autocommit = True
    try:
        with conn.cursor() as cur:
            # role_name/target_db come from CLI args and are real SQL
            # identifiers (role/database names), not data -- %s
            # parameterization only covers string literals (like the
            # password below), so identifiers are built via
            # psycopg2.sql.Identifier instead of an f-string, matching
            # this codebase's own ban on interpolating untrusted text
            # directly into SQL.
            password = secrets.token_urlsafe(32)
            cur.execute("SELECT 1 FROM pg_roles WHERE rolname = %s", (role_name,))
            if cur.fetchone():
                print(f"Role '{role_name}' already exists -- rotating its password.")
                cur.execute(
                    sql.SQL("ALTER ROLE {} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %s").format(
                        sql.Identifier(role_name)
                    ),
                    (password,),
                )
            else:
                print(f"Creating role '{role_name}'.")
                cur.execute(
                    sql.SQL("CREATE ROLE {} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %s").format(
                        sql.Identifier(role_name)
                    ),
                    (password,),
                )
            # The ONLY privilege this role needs: permission to open a
            # connection to the target database. Deliberately no schema
            # USAGE, no table SELECT/INSERT/UPDATE/DELETE grants of any
            # kind -- LISTEN/NOTIFY and constant-expression queries like
            # `SELECT 1` require none of that.
            cur.execute(
                sql.SQL("GRANT CONNECT ON DATABASE {} TO {}").format(
                    sql.Identifier(target_db), sql.Identifier(role_name)
                )
            )
            print(f"Granted CONNECT-only access on '{target_db}' to '{role_name}'.")
    finally:
        conn.close()
    return password


def write_env_file(path, role_name, password, admin_host, admin_port, target_db):
    directory = os.path.dirname(path)
    os.makedirs(directory, mode=0o700, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as env_file:
        env_file.write("# Auto-generated by provision_cache_manager_db_role.py\n")
        env_file.write("# Restricted Postgres role for cache_manager.py: CONNECT-only, no table access.\n")
        env_file.write(f"DB_HOST={admin_host}\n")
        env_file.write(f"DB_PORT={admin_port}\n")
        env_file.write(f"DB_NAME={target_db}\n")
        env_file.write(f"DB_USER={role_name}\n")
        env_file.write(f"DB_PASS={password}\n")
    print(f"Wrote credentials to {path} (0600).")


def main():
    args = parse_args()
    password = provision(
        args.admin_host, args.admin_port, args.admin_user, args.admin_password,
        args.target_db, args.role_name,
    )
    write_env_file(args.env_file, args.role_name, password, args.admin_host, args.admin_port, args.target_db)
    print("Done. Restart the cache_manager.py daemon to pick up the new credentials.")


if __name__ == "__main__":
    sys.exit(main())
