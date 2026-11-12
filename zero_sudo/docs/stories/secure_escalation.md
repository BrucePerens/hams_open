<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Secure Privilege Escalation `[@ANCHOR: zero_sudo:COMM_story_secure_escalation]`

This story describes how developers securely escalate privileges without using the dangerous `.sudo()` command.

## Background
In the Zero-Sudo architecture, direct use of `.sudo()` is prohibited. Instead, background tasks must be executed by specific service accounts.

## The Process
1. **Developer Identification**: The developer identifies a need for elevated privileges (e.g., creating a record that the current user shouldn't have access to).
2. **Service Account Retrieval**: The developer calls `_get_service_uid` (entry below) to get the ID of a pre-defined service account.
3. **Impersonation**: The developer uses `.with_user(svc_uid)` to execute the specific operation as the service account.

## Example
```python
svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('my_module.my_service_user')
self.env['my.model'].with_user(svc_uid).create({'name': 'Secure Record'})
```

All the functions below are methods of the abstract model `zero_sudo.security.utils` in `zero_sudo/models/security_utils.py`, decorated
`@api.model` (call them on `self.env['zero_sudo.security.utils']`). The Postgres functions are installed from
`zero_sudo/data/postgres_procedures.xml` on every module update.

### _get_service_uid `[@ANCHOR: zero_sudo:COMM_get_service_uid]`

**Purpose.** Turns the xmlid of a service account (`module.name`, a `res.users` record shipped by a module) into its user id, after
proving that the account is a safe, narrow service account. It replaces `.sudo()`, which is forbidden in this codebase, as the only way a
module acts with more rights than the current user.

**Signature.** `_get_service_uid(xml_id) -> int`

| Argument | Type | Meaning |
|---|---|---|
| `xml_id` | `str` | `module.name` of a `res.users` record. Anything else (empty, not a string, no dot) is refused. |

**Returns.** The integer `res.users` id. Pass it to `.with_user(uid)`.

**Errors.** `odoo.exceptions.AccessError` for a malformed xmlid ("Invalid XML ID format") and for every failure of the SQL checks below
(not found, disabled, not a service account, holds a global administrative group). The database's `RAISE EXCEPTION` surfaces as
`psycopg2.errors.RaiseException`; the function catches it and re-raises it as `AccessError` so that every caller's `except AccessError`
works. Nothing else is raised for those cases.

**Locking, caching and side effects.** Before the query the function calls `self.env.flush_all()` (so a pending ORM write to the
service user's `active` flag or groups is seen by the SQL) and runs the query inside a savepoint opened with `flush=False`: the
savepoint is required, because a raw `RAISE EXCEPTION` aborts the whole transaction and without a rollback point the caller's next
statement would fail with `InFailedSqlTransaction`. The `flush=False` avoids Odoo's "Missing default_env" public-user lookup on every
public request. The xmlid-to-uid resolution is cached per worker in `_SERVICE_UID_CACHE`, keyed by `(dbname, xml_id)` and filled at registry
load by `_preload_service_uid_cache`; on a cache hit only the live verification runs, on a miss the full resolve-and-verify runs and the
result is deliberately NOT written back (a cache is not transaction scoped, and a rolled-back test would leak a stale uid). The safety
verdict is never cached: it is re-run on every call, so disabling an account or granting it a group takes effect at once. No writes.

**Security.** The three SQL checks are the enforcement; the Python side adds no trust. See the next four entries. A caller must name the
account literally; never build the xmlid from user input.

**See also.** Tests `zero_sudo/tests/test_security_utils.py` (`COMM_test_get_service_uid`); the service account lifecycle journey in
`zero_sudo/docs/journeys/service_account_lifecycle.md`.

## Security Enforcement

`_get_service_uid` (above) ensures four things, each decided inside Postgres in `zero_sudo_get_service_uid(xml_id)`, which is
`zero_sudo_resolve_service_xmlid` followed by `zero_sudo_verify_service_uid`.

### Resolution by raw SQL `[@ANCHOR: zero_sudo:COMM_get_service_uid_sql_resolve]`

`zero_sudo_resolve_service_xmlid(p_xml_id TEXT) RETURNS INTEGER` splits the xmlid on the first dot into module and name, and selects
`res_id` from `ir_model_data` where `model = 'res.users'`. If no row exists it raises `Security Alert: Service Account <xmlid> not found.`
and returns nothing, so an xmlid that names some other model's record (a group, a partner) is "not found" and cannot be used. It
runs as plain SQL, so ORM interception, ORM caching and record rules cannot change the answer. Python calls
`SELECT zero_sudo_get_service_uid(%s)` on a cache miss and takes the first column; see the caching note in `_get_service_uid`.
Test: `COMM_test_get_service_uid_sql_resolve`.

### Verification: active and a service account `[@ANCHOR: zero_sudo:COMM_get_service_uid_sql_verify]`

`zero_sudo_verify_service_uid(p_uid INTEGER, p_xml_id TEXT) RETURNS VOID` first reads `active` and `is_service_account` from `res_users`
for the uid. If `active` is not true it raises `Security Alert: Service Account <xmlid> is disabled.`; if `is_service_account` is not
true it raises `<xmlid> is a human user, not a Service Account.` The row is read at call time, so a deactivated account cannot be
used even if its uid is cached, and a human user cannot be impersonated by naming their xmlid. On a cache hit Python calls
`SELECT zero_sudo_verify_service_uid(%s, %s)` with the cached uid. Test: `COMM_test_get_service_uid_sql_verify`.

### The service account flag `[@ANCHOR: zero_sudo:COMM_is_service_account_field]`

`res.users.is_service_account` is a `Boolean` field (default `False`, label "Is Service Account", restricted with
`groups="base.group_system"`) defined in `zero_sudo/models/res_users.py`. Module data sets it `True` on the service accounts they ship; only
an administrator can write it. It does three jobs: the SQL verification above requires it, interactive web login is refused for any user
with it set (the login-blocking story), and `ResUsersZeroSudo._service_accounts_among_self` reads it with SQL because the field's group
restriction would make an ORM filter raise `AccessError` for a narrower caller. Only officially designated accounts can proceed. Tests:
`COMM_test_is_service_account_field`.

### No global administrative groups `[@ANCHOR: zero_sudo:COMM_privilege_escalation_block_sql]`

The last step of `zero_sudo_verify_service_uid`: a recursive query walks the account's groups from `res_groups_users_rel` through
`res_groups_implied_rel` (the transitive closure Odoo itself uses for `has_group`) and looks for `base.group_system` or
`base.group_erp_manager` (resolved through `ir_model_data`). If found it raises `Service Account <xmlid> violates the Zero-Sudo mandate by
possessing global administrative groups.` Checking only direct membership would let an account hold an admin group through an implied
chain and defeat the whole mandate. A service account is therefore always a narrow account. Test: `COMM_test_privilege_escalation_block_sql`.

### _get_service_env `[@ANCHOR: zero_sudo:get_service_env]`

**Purpose.** Returns a whole `Environment` that already runs as the named service account, for callers that need more than a bare uid
(model lookups, several writes, calling other modules' services).

**Signature.** `_get_service_env(xml_id, context=None) -> Environment`

| Argument | Type | Meaning |
|---|---|---|
| `xml_id` | `str` | A service account xmlid, checked exactly as in `_get_service_uid`. |
| `context` | `dict` or `None` | Extra context keys merged last, so they override the defaults below. Borrowed, not modified. |

**Returns.** A new `Environment` for that uid. Its context is the caller's with `allowed_company_ids` removed (the caller's company scope
may not be the service account's; ADR 0083 forbids injecting that key by hand), then switched with `.with_company()` to the account's own
company, then `mail_notrack=True` set so background writes do not create chatter or followers, then `context` applied.

**Errors.** Whatever `_get_service_uid` raises (`AccessError`).

**Special concerns.** Runs the live service-account verification on every call; no writes and no locks of its own; the returned
environment shares the caller's cursor and transaction. Used for example by `ir_module_module.py` to act as the facility service
account.

### _ensure_executable `[@ANCHOR: zero_sudo:ensure_executable]`

**Purpose.** Finds a system binary a daemon or module needs (for example `kopia` for backups), and when it is not installed fetches it
through the binary-manifest downloader as a service account, rather than failing outright or silently degrading.

**Signature.** `_ensure_executable(cmd_name, svc_xml_id=None, pkg_name=None) -> str`

| Argument | Type | Meaning |
|---|---|---|
| `cmd_name` | `str` | Executable name searched with `shutil.which`. |
| `svc_xml_id` | `str` or `None` | Service account allowed to download; when `None` no download is attempted. |
| `pkg_name` | `str` or `None` | Package name used in the error message; defaults to `cmd_name`. |

**Returns.** The absolute path of the executable: from `PATH` when present, else whatever `binary.manifest.ensure_executable(cmd_name)`
returns when run in `_get_service_env(svc_xml_id)`.

**Errors.** `UserError("Missing dependency ...")` naming the `apt-get install` package when it is not on `PATH` and no service account is
given; `AccessError` from the service account check; the downloader's own errors (the `binary_downloader` module, not documented here).

**Special concerns.** May download and write a file through the service account, and may take a long time; do not call it inside a
request that must be quick. It never uses `sudo` or installs OS packages.

## Declared Cross-Module Dependencies, Without a Hard `depends`

Some modules need to call into another module's own code only when that OTHER module happens to be installed, without adding a hard manifest dependency that would close a real dependency cycle (see each such module's own `depends_cycle` manifest key; `hams_shared/tools/check_dependency_cycles.py` checks that each entry is honest).

### _resolve_dependency_cycle `[@ANCHOR: zero_sudo:resolve_dependency_cycle]`

**Signature.** `_resolve_dependency_cycle(dependency_module, required=False) -> bool`

| Argument | Type | Meaning |
|---|---|---|
| `dependency_module` | `str` | Technical name of the module whose installation state is asked. |
| `required` | `bool` | When true, raise instead of returning False for an absent module; leave false when the caller can degrade and log why. |

**Returns.** `True` when the module is in `self.env.registry._init_modules` (installed in this registry), else `False`.

**Errors.** `UserError` when the calling module cannot be identified, when the calling module's `__manifest__.py` does not list
`dependency_module` under `depends_cycle`, and, if `required`, when the module is not installed. The first two are the caller's bug and
fail fast; they are never treated as "dependency missing".

**Security and special concerns.** The caller's module is resolved from the real Python call stack by `_caller_module_name` and not
from an argument, so code cannot probe an arbitrary undeclared module. The manifest is therefore the one place the relationship is
written down. Reads the manifest through Odoo's `get_manifest`, which is cached; no database access, no locks.

### _caller_module_name `[@ANCHOR: zero_sudo:caller_module_name]`

`_caller_module_name() -> str | None` returns the technical name of the module that owns the code that called into
`_resolve_dependency_cycle`. It walks `inspect.stack()` skipping frames in `security_utils.py` itself, and from the first other frame's file it
walks up the directory tree to the nearest directory holding a `__manifest__.py`, returning that directory's name. It returns `None`
when a frame is not a real file (`<stdin>`, `<string>`, `odoo shell`), tested before `abspath` so that a pseudo-name cannot be resolved
against the working directory and be attributed to whatever module the cwd is in, and when no manifest is found. `None` makes
`_resolve_dependency_cycle` fail. It is a stack walk, so it is meant for the immediate-caller pattern only (a wrapper function in another
module would be treated as the caller).
