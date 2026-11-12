# Hams AI Safety Reporting (`hams_ai_safety_reporting`)

Adds the `ai_safety_concern` ticket type to `hams_helpdesk.ticket` and a narrow, create-only service account,
`hams_ai_safety_reporting.user_ai_safety_reporting_service`, that the Simulated Band bots and the Official Observer use to file a
safety ticket (`docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md` in hams_com, section G, Phase 7). The account's design is
documented in `security/security.xml`; this file documents the daemon key it uses.

## `register_ai_safety_reporting_key(env)`: the account's daemon key [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]

`from odoo.addons.hams_ai_safety_reporting.hooks import register_ai_safety_reporting_key`

The daemons that file a safety ticket authenticate as the service account with an API key in
`/opt/hams/etc/keys/ai_safety_reporting_service_internal.env` (`hooks.KEY_FILE`; the units `hams.simulated.bots` and
`hams.simulated.observer` name it in `ODOO_KEY_FILE`). Before version 1.1 nothing in code created that key: its Daemon Key Manager
registry row was added by hand on hams.com, so a new install never had the key and a flagged contact could not become a ticket. This
function registers the row, so the key is issued and rotated like every other daemon key (every 59 days; the bots read the file again
for each ticket, so a rotation costs them nothing). `post_init_hook` calls it on a fresh install and `migrations/1.1` calls it once on
an installed database (`migrate(cr, version)`, documented last).

| Argument | Type and range | Meaning |
|---|---|---|
| `env` | `odoo.api.Environment` | Any environment of the database. The function resolves the service account and the Daemon Key Manager's service account by their external ids (`zero_sudo.security.utils`); nothing runs as the superuser. |

| Result | Meaning |
|---|---|
| `None` | The registry row exists, holds the service account and `KEY_FILE`, and a fresh key was minted and written. |
| `odoo.exceptions.AccessError` (raised) | One of the two service accounts is missing or not a service account; the install or upgrade stops. |
| `odoo.exceptions.UserError` / `ValidationError` (raised) | The registry refused the row (path outside `/opt/hams/etc/keys/`, or an archived account). |

Behaviour worth knowing: every call mints a new key and writes the file (it revokes the previous key of this row), so run it from an
install or an upgrade, not from a loop. It is idempotent in rows: a registry row that already holds `KEY_FILE` is updated in place and
keeps its own name (`register_daemon` looks a row up by name, and the path is unique, so a second name for the same path would be
refused: hams.com's hand-made row is therefore adopted, whatever it is called). A new row is named `KEY_NAME`, "AI Safety Reporting".
The file is written `odoo:odoo` mode `0600` because both units still run as `odoo`; moving them to a family account (and the key into a
family key directory, `os_group`) is a later step of `docs/proposals/DAEMON_OS_ISOLATION_PLAN.md`. No data is read from a ticket. Tests:
`tests/test_ai_safety_reporting_key.py` (the row, the key, the file mode and content, a second call, and a hand-made row under another
name).

## `post_init_hook(env)`: registering the key on a fresh install [@ANCHOR: hams_ai_safety_reporting:post_init_hook]

`hooks.post_init_hook`, named in the manifest

Odoo calls it once, after the module's data has loaded on a **fresh install** (never on an upgrade; `migrations/1.1` covers an installed
database). It does one thing: it calls `register_ai_safety_reporting_key(env)` (above), so a new site has the key the bots and the
observer file safety tickets with, from the moment the module is installed.

| Argument | Type and range | Meaning |
|---|---|---|
| `env` | `odoo.api.Environment` | The installing environment Odoo passes; any user. Nothing runs as the superuser. |

| Result | Meaning |
|---|---|
| `None` | The key row was registered and the key file written, exactly as `register_ai_safety_reporting_key` returns. |
| any exception from `register_ai_safety_reporting_key` | Passes through unchanged, so the install fails loudly instead of leaving the bots without a key. |

Behaviour worth knowing: it writes `/opt/hams/etc/keys/ai_safety_reporting_service_internal.env` as a side effect, so a test host that
installs the module gets that file (the same is true of every module that registers a daemon key). It is not idempotent in effect (each
call mints a new key) but it runs only once per install. Tests: `tests/test_ai_safety_reporting_key.py`.

## `migrate(cr, version)`: registering the key on an installed database [@ANCHOR: hams_ai_safety_reporting:migrate]

`migrations/1.1/post-register-ai-safety-reporting-key.py`, run by Odoo's upgrade machinery

Version 1.0 never registered the service account's daemon key (it was added by hand on hams.com), and `post_init_hook` runs only on a fresh install, so an already installed database gets the key here when it is upgraded to 1.1. It looks up the service account (`hooks.SERVICE_XMLID`) in `ir_model_data` with a plain SQL query, then calls `register_ai_safety_reporting_key` with an environment for that user. Odoo calls it once per upgrade past 1.1; it takes no other action.

| Argument | Type and range | Meaning |
|---|---|---|
| `cr` | database cursor | The upgrade's cursor, inside the upgrade transaction. |
| `version` | str or `None` | The previously installed version; `None` or empty on a fresh install. |

| Result | Meaning |
|---|---|
| `None` (nothing done) | `version` is empty: a fresh install, where `post_init_hook` does the work. |
| `None` (key registered) | The registry row exists or was updated, and a fresh key was minted and written, exactly as `register_ai_safety_reporting_key` does. |
| `ValueError` (raised) | The service account's external id is not in `ir_model_data`; the upgrade stops. |
| any exception of `register_ai_safety_reporting_key` | Passes through, so the upgrade fails rather than leaving the bots without a key. |

Behaviour worth knowing: a row already holding the key file is kept under its own name (see above), so the migration is safe on hams.com's hand-made row. Each run mints a new key and writes `/opt/hams/etc/keys/ai_safety_reporting_service_internal.env`, so it must not be run repeatedly. The environment is the service account's own, not the superuser's. Tests: `tests/test_ai_safety_reporting_key.py` (`test_the_migration_registers_the_key_on_an_installed_database_only`: no version does nothing, version 1.0 registers the key, a second run updates the same row); the `ValueError` branch for a missing account has no direct test (to-do: add one).
