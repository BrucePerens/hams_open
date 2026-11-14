<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Tests Run As the Users of Production

As a developer, I want a test to run code as the people who run it on hams.com, so that an access rule, an ACL (access control list) row or a cron cannot pass the suite
while failing for every real user. The decision and the incidents behind it are in `hams_shared/docs/adrs/0111_tests_run_as_the_personas_of_production.md`.

All of it is in `zero_sudo/tests/test_persona_support.py` (a support module, not a test file; it is imported by the test base classes
and by tests that use `@personas`). `PersonaMixin` is mixed into `HamsTransactionCase` and `HamsHttpCase`. Everything below is test
support: it runs only inside a test database and creates users that are rolled back.

## Data

`CATALOGUE` is a dict of persona key to a spec dict: `kind` (`public`, `portal`, `internal`), `groups` (xmlids), optional
`groups_if_installed` (xmlids added only when their module is installed, for example the user_websites group on portal members),
`website` (`"none"` or `"own"`, the value wanted for `res.users.website_id`) and `prod` (what was counted in production on
2026-10-26). Keys: `public` (the existing `base.public_user`), `portal_member`, `internal_staff`, `admin`, `pager_admin`,
`helpdesk_staff`, plus a derived `<key>_with_website` for every portal key (website `"own"`), because a multi-tenant install binds a login
to a website while hams.com binds none. Internal users are never bound to a website: Odoo refuses it. The module-level
`_persona_counter` numbers the logins (`persona_<key>_<n>`) so that several personas in one test never collide. `PersonaError` is an
`AssertionError`, so a test that asks for an impossible persona fails and is never skipped.

### Persona `[@ANCHOR: zero_sudo:persona_object]`

**Purpose.** The value a test body receives: one user of the site, the environment to act as that user, and what the test expects of it.
`Persona(key, user, password=None, allowed=None)` is built by the functions below, rarely by hand.

| Field | Type | Meaning |
|---|---|---|
| `key` | `str` | Catalogue key, or `service:<xmlid>`, `cron:<name>`, or a derived name such as `internal_staff+<group full name>`. Used as the `subTest` label. |
| `user` | `res.users` | The user record (superuser environment, so it can be read in assertions). |
| `password` | `str` or `None` | Set only for a built catalogue persona, so the HTTP session can log in. `None` for the public user, services and crons. |
| `allowed` | `bool` or `None` | What the test expects: `True` the persona may do the thing, `False` must be refused, `None` not declared. Set by the chooser or the caller. |
| `env` | `Environment` | `user.with_user(user).env`: the environment whose ORM calls run with the persona's own access rights. |

`__repr__` prints key, login and `allowed`. The object is plain data with no locks and no database writes of its own; it lives for
one test body and is dropped with the savepoint.

## Building a persona

### build_persona `[@ANCHOR: zero_sudo:persona_build]`

**Purpose.** Creates the catalogue user `key` as production has it, so a test does not invent a user that nobody is.

**Signature.** `build_persona(env, key, allowed=None, extra_group=None) -> Persona`

| Argument | Type | Meaning |
|---|---|---|
| `env` | `Environment` | Test environment; the user is created through it. |
| `key` | `str` | A `CATALOGUE` key. |
| `allowed` | `bool` or `None` | Stored on the persona. |
| `extra_group` | `res.groups` record or `None` | One more group for the user; used by `model_personas` for a group no catalogue persona holds. |

**Returns.** A `Persona`. For `public` it is the existing `base.public_user` with no password and no creation. Otherwise a new user named
`persona_<key>_<n>` with a generated password, the spec groups, those of `groups_if_installed` that exist, and `extra_group`;
`website_id` is set to False, or to a newly created `website` record for `*_with_website` personas; `website_slug` (the user_websites
member site) is set when the field exists. If Odoo filled a default website into an unbound user, it is written back to False.

**Errors.** `PersonaError` for an unknown key (the message lists the catalogue) and for a prerequisite the database lacks (a group
whose module is not installed, or the `website` model for a bound persona); the test fails and names the module to install.

**Special concerns.** Writes a `res.users` row (and a `website` row for bound personas) with `no_reset_password`, so no mail is sent.
It does not roll back: the caller is the `@personas` runner, whose savepoint does.  The password is for the test database only.

**See also.** `zero_sudo/tests/test_personas.py` and `test_persona_sweeps.py`.

### service `[@ANCHOR: zero_sudo:persona_service]`

`service(env, xmlid, allowed=None) -> Persona` returns the shipped service account `xmlid` exactly as its module installs it, with
its real groups, website and share flag, and no password. A service account is a data record from its module and is never copied:
production runs exactly that user, so a test of what a daemon's account may do must use the record. It raises `PersonaError` when the
xmlid does not resolve or resolves to a model other than `res.users`. It reads only and does not create anything. Used through
`@personas(services=(...))`.

### cron `[@ANCHOR: zero_sudo:persona_cron]`

`cron(env, cron_ref, allowed=None) -> Persona` is the user a scheduled job really runs as. `cron_ref` is an xmlid string or an
`ir.cron` record; the persona is named `cron:<cron_name>` and wraps the cron's `user_id`, with no password. A string that does not
resolve raises the ORM's `ValueError` from `env.ref`. Used through `@personas(cron_ref=...)` and as the subject of the two cron
helpers below. It does not run anything.

## Choosing personas from what is under test

### route_personas `[@ANCHOR: zero_sudo:persona_route_derivation]`

**Purpose.** For a controller route, builds who may call it and who must be refused, from the route's `auth=` and the groups its code checks.

**Signature.** `route_personas(env, auth, groups=()) -> list[Persona]`

| Argument | Type | Meaning |
|---|---|---|
| `env` | `Environment` | Test environment. |
| `auth` | `str` | `"none"`, `"public"`, `"user"` or `"bearer"`. |
| `groups` | iterable of xmlids | Groups the controller requires; empty means any logged-in user. |

**Returns.** `none`: the public persona, allowed. `public`: public, portal member, internal staff and admin, all allowed. `user` or `bearer`:
the public persona refused, then portal member, internal staff and admin, each allowed when no group is named or when it holds one of the
named groups (implied groups count, through `all_group_ids`).

**Errors.** `PersonaError` for any other `auth`, or when a named group is not installed.

**Special concerns.** Creates users through `build_persona`; call it inside a savepoint (the decorator does). It cannot check a rule enforced
only in the controller code beyond the groups it is told about.

### model_personas `[@ANCHOR: zero_sudo:persona_model_derivation]`

**Purpose.** For a model, derives the users who must be allowed and some who must be refused, from the model's own ACL rows and record rules,
so the test follows the access data instead of a hand-written list.

**Signature.** `model_personas(env, model_name, mode="read") -> list[Persona]`

| Argument | Type | Meaning |
|---|---|---|
| `env` | `Environment` | Test environment. |
| `model_name` | `str` | Technical model name, for example `res.partner`. |
| `mode` | `str` | `read`, `write`, `create` or `unlink`; selects the `perm_<mode>` columns. |

**Returns.** The named groups are every group of an `ir.model.access` row granting the mode plus every group of an `ir.rule` for the model
and mode. Each catalogue persona whose groups include one of them is returned allowed. A named group that no catalogue persona holds gets an
`internal_staff` user with exactly that group (key `internal_staff+<group>`), allowed. Then each of `public`, `portal_member` and
`internal_staff` not already returned is built and returned only if it is refused (it holds no named group and the model has no
unconditional ACL row).

**Errors.** `PersonaError` when the model has no ACL row and no rule for the mode: nothing can be derived, so the test must name personas itself.

**Special concerns.** Reads `ir.model.access` and `ir.rule` and creates one user per persona; run inside a savepoint. Catalogue entries whose
module is missing are skipped silently here (unlike `build_persona`).

### personas `[@ANCHOR: zero_sudo:persona_decorator]`

**Purpose.** The decorator that makes a test run as production users: it runs the decorated test once per chosen persona.

**Signature.** `personas(*keys, model=None, mode="read", cron_ref=None, route_auth=None, route_groups=(), services=())`, used as
`@personas(...)` on `def test_x(self, persona)`.

| Argument | Type | Meaning |
|---|---|---|
| `*keys` | `str` | Catalogue keys, each built allowed. |
| `model`, `mode` | `str` | Derive from ACLs with `model_personas`. |
| `cron_ref` | `str` or `ir.cron` | Add the cron's user. |
| `route_auth`, `route_groups` | `str`, iterable | Derive from the route with `route_personas`. |
| `services` | iterable of xmlids | Add each service account. |

**Returns.** A decorator; the wrapped method takes the test's own arguments and passes `persona` as the first argument after `self`.

**Errors.** `PersonaError` at decoration time when nothing is named (so a test can never run as the superuser by omission), and at run time
when the derivation yields no persona. The first failing persona fails its `subTest`; others still run.

**Special concerns.** Each persona runs through the savepoint runner below, so its data changes never leak. It needs a test case with
`PersonaMixin`.

### _run_in_rolled_back_savepoint `[@ANCHOR: zero_sudo:persona_savepoint_runner]`

`PersonaMixin._run_in_rolled_back_savepoint(self, test, persona, *args, **kwargs)` calls `test(self, persona, ...)` inside
`env.cr.savepoint()` and then raises the private `_Rollback` exception on purpose, so the savepoint is always rolled back, and swallows
that exception and calls `env.invalidate_all()` so the ORM cache holds nothing from the discarded work. Any other exception from the test
propagates (after the savepoint rolls back) to the caller's `subTest`. Returns nothing. It does not open a new transaction and it must run
in a test case that owns a cursor; nothing is committed.

## Asserting and logging in

### assert_persona_access `[@ANCHOR: zero_sudo:persona_assert_access]`

`assert_persona_access(self, persona, records, mode="read")` compares the real answer of `records.with_user(persona.user).has_access(mode)`
with `persona.allowed` and fails with a message naming the persona, mode and records. `records` is a recordset, `mode` one of
`read`, `write`, `create`, `unlink`. It reads only. A persona with `allowed=None` is expected to be refused (`bool(None)` is False), so
declare `allowed` explicitly.

### authenticate_as `[@ANCHOR: zero_sudo:persona_login]`

`authenticate_as(self, persona)` logs the HTTP test session in as the persona with its generated password and returns what Odoo's
`authenticate` returns. A persona without a password (public, a service, a cron) raises `PersonaError`. It changes the session cookie of
the `HamsHttpCase` client, so later requests in that test run as the persona.

## Crons

### run_cron_action `[@ANCHOR: zero_sudo:persona_run_cron_action]`

`run_cron_action(env, cron_ref) -> result of the action` runs the cron's server action as the cron's own `user_id` the way `ir.cron`
does: `record.ir_actions_server_id.with_user(record.user_id).run()`. `cron_ref` is an xmlid or an `ir.cron` record. The call matters
because `ir.actions.server.run()` first refuses a user without write access to the action's model; calling the model method directly skips
that check, which is how a cron that Odoo had switched off passed its test. It really runs the action, with its side effects on the
database and on any network the action reaches, so use `cron_refusal` for a cron that fetches from a third party. Errors are the
action's own, including `AccessError`.

### cron_refusal `[@ANCHOR: zero_sudo:persona_cron_refusal]`

`cron_refusal(cron_record) -> str | None` returns the `AccessError` text Odoo would raise before running the cron's action as its user,
or `None` when it would be allowed. It calls `_can_execute_action_on_records` with the empty recordset that `run()` passes, for the
action's `model_name`, and does not run the action, so a test machine never contacts a third-party server. `cron_record` is an `ir.cron`
record. Read-only; any other exception propagates.

## Sweeps

### in_repository `[@ANCHOR: zero_sudo:persona_repository_modules]`

`in_repository(module, member) -> bool` is True when the installed module `module` lives in the same addons directory as `member`,
comparing the parent directory of `get_module_path`. A module with no path gives False. The sweeps pass the module of the test (such as
`zero_sudo`) as `member`, so that hams_open and hams_com each sweep only their own modules and not each other's. Pure path check.

### cron_sweep `[@ANCHOR: zero_sudo:persona_cron_sweep]`

`cron_sweep(env, repository=None) -> list[str]` returns one readable line per installed `ir.cron` whose own user would be refused:
`<module>.<name> (as <login> on <model>): <reason>`. `repository` is a module name; when given only crons of modules in its repository
(`in_repository`) are checked. It walks every `ir.model.data` row of model `ir.cron`, includes inactive crons, skips rows whose record is
gone, and uses `cron_refusal`, so nothing is executed. An empty list means every cron passes the gate; a test asserts that.

### rule_user_attributes `[@ANCHOR: zero_sudo:persona_rule_attributes]`

`rule_user_attributes(env, rule) -> set[str]` returns the names of fields of `res.users` that the rule's `domain_force` reads as
`user.<name>`, limited to real, non-required fields other than `id`, since only an optional field can be empty. For the rule
`[('website_id','=',user.website_id.id)]` it returns `{'website_id'}`. Matching is by regular expression on the domain text, so an
attribute reached through another expression is not seen. Pure.

### rule_empty_attribute_collapse `[@ANCHOR: zero_sudo:persona_rule_empty_attribute]`

**Purpose.** Detects the production bug where a record rule compares a field with `user.<attribute>` and the attribute is empty for every real
user, so the rule matches only records with no value and hides the rest.

**Signature.** `rule_empty_attribute_collapse(env, rule, attribute) -> bool | None`. `rule` is an `ir.rule`; `attribute` a `res.users`
field name of type many2one or char.

**Returns.** `True` when, evaluating `domain_force` for a probe user with the attribute empty and one with it set, a leaf that holds a
value for the second holds an empty value for the first. `False` when the domain changes shape (the null-safe form that adds the test only
when the attribute is set) or no leaf collapses. `None` when it cannot decide: another field type, or no record to point a many2one at
(a missing `website` record is created), or no probe user can be made.

**Special concerns.** Creates two probe users in savepoints (`_probe_user` tries the rule's groups together and then each alone, because a
rule naming the portal and public groups cannot be held by one user) and evaluates the domain with `safe_eval`. It may create a `website`
row. Call inside a rolled-back savepoint.

### rule_sweep `[@ANCHOR: zero_sudo:persona_rule_sweep]`

`rule_sweep(env, repository) -> dict[str, list[str]]` returns `{ "<module>.<rule xmlid name>": [attributes] }` for the record rules of the
modules in the repository of module `repository` that collapse for an empty attribute, using `rule_user_attributes` and
`rule_empty_attribute_collapse`. An empty dict means none. A finding must be fixed or listed in the test as deliberate with the reason,
as the story above says. It creates probe users per rule and attribute, so run it in a rolled-back test.

### cron smoke run `[@ANCHOR: zero_sudo:cron_runs_clean_as_its_user]`

`zero_sudo/tests/test_cron_runs_clean.py` runs the server action of every installed non-core `ir.cron` as the cron's own
user (inside a savepoint) and fails when it raises or when any WARNING or ERROR log line mentions an access refusal
(`AccessError`, "not allowed to access"), which catches a method that catches and logs the refusal so the mail is never sent.
It sees only crons of installed modules, so the whole-system run covers every module.
