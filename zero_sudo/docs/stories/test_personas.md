<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Tests Run As the Users of Production

As a developer, I want a test to run code as the people who run it on hams.com, so that an access rule, an ACL row or a cron cannot pass the suite
while failing for every real user. The decision and the incidents behind it are in `hams_shared/docs/adrs/0111_tests_run_as_the_personas_of_production.md`.

## The Catalogue
- **Persona** `[@ANCHOR: zero_sudo:persona_object]` **/ Build** `[@ANCHOR: zero_sudo:persona_build]`: a user, its environment and what the test expects of it. `build_persona(env, key)` creates the catalogue user: the public visitor, a portal member with or without a website, internal staff, an administrator with no website, a pager admin or a helpdesk manager. Each carries the groups and the (empty) `user.website_id` that the production users of that kind have, with the counts they were read from. This repository is open source and meant for multi-tenant installs where a login is bound to its website, so every portal persona also exists as `<key>_with_website` (Odoo refuses to bind an internal user to a website, so internal personas exist only unbound); the model derivation and the sweeps run both. A persona whose module is not installed fails the test.
- **Service account** `[@ANCHOR: zero_sudo:persona_service]` **/ Cron user** `[@ANCHOR: zero_sudo:persona_cron]`: the installed user record itself, with its real groups; a cron's persona is the cron's own `user_id`.

## Choosing Personas From What Is Under Test
- **Model** `[@ANCHOR: zero_sudo:persona_model_derivation]`: a user in each group the model's ACL rows and record rules name for the access mode, plus the public, portal and internal users who must be refused. A model with no ACL row for the mode derives nothing and the test fails.
- **Route** `[@ANCHOR: zero_sudo:persona_route_derivation]`: from `auth=` (`none`, `public`, `user`) and the groups the controller checks; the users without the group are the ones who must be refused.
- **Runner** `[@ANCHOR: zero_sudo:persona_decorator]` **/ Savepoint** `[@ANCHOR: zero_sudo:persona_savepoint_runner]`: `@personas(...)` runs the test body once per persona, each in a savepoint that is rolled back and its own `subTest`. A test that names no persona fails.
- **Helpers** `[@ANCHOR: zero_sudo:persona_assert_access]` `[@ANCHOR: zero_sudo:persona_login]`: `assert_persona_access` compares the real ORM answer with what the persona is declared to be allowed; `authenticate_as` logs the HTTP session in as a catalogue persona.

## Crons
- **Run the action** `[@ANCHOR: zero_sudo:persona_run_cron_action]`: runs the cron's server action as the cron's user, with the write-access check of `ir.actions.server.run()`. Calling the model method as that user skips the check, which is how a cron that Odoo switched off passed its test.
- **The gate** `[@ANCHOR: zero_sudo:persona_cron_refusal]`: the same check without running the action, because a cron may fetch from a third-party server.

## Sweeps
- **Crons** `[@ANCHOR: zero_sudo:persona_cron_sweep]`: every installed cron passes the gate as its own user.
- **Record rules** `[@ANCHOR: zero_sudo:persona_rule_attributes]` `[@ANCHOR: zero_sudo:persona_rule_empty_attribute]` `[@ANCHOR: zero_sudo:persona_rule_sweep]`: a rule whose domain reads an optional attribute of `user` is evaluated for a user with the attribute empty and one with it set. A rule that then tests records against the empty value hides them from every production user (none has a website) and is listed, with the reason, as deliberate or as an open finding.
- **Repository modules** `[@ANCHOR: zero_sudo:persona_repository_modules]`: which installed modules belong to this repository, so hams_open and hams_com each sweep their own.
