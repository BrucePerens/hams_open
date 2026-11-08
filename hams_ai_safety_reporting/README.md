# Hams AI Safety Reporting (`hams_ai_safety_reporting`)

Adds the `ai_safety_concern` ticket type to `hams_helpdesk.ticket` and a narrow, create-only service account,
`hams_ai_safety_reporting.user_ai_safety_reporting_service`, that the Simulated Band bots and the Official Observer use to file a
safety ticket (`docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md` in hams_com, section G, Phase 7). The account's design is
documented in `security/security.xml`; this file documents the daemon key it uses.

## `register_ai_safety_reporting_key(env)`: the account's daemon key

`from odoo.addons.hams_ai_safety_reporting.hooks import register_ai_safety_reporting_key` [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]

The daemons that file a safety ticket authenticate as the service account with an API key in
`/opt/hams/etc/keys/ai_safety_reporting_service_internal.env` (`hooks.KEY_FILE`; the units `hams.simulated.bots` and
`hams.simulated.observer` name it in `ODOO_KEY_FILE`). Before version 1.1 nothing in code created that key: its Daemon Key Manager
registry row was added by hand on hams.com, so a new install never had the key and a flagged contact could not become a ticket. This
function registers the row, so the key is issued and rotated like every other daemon key (every 59 days; the bots read the file again
for each ticket, so a rotation costs them nothing). `post_init_hook` calls it on a fresh install and `migrations/1.1` calls it once on
an installed database.

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
