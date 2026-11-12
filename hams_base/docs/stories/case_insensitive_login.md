<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Signing In Does Not Depend on How My Phone Capitalised My Address

As a **Member** who signs in with the email address I registered, I want it to work however my keyboard capitalised it
(`N2YCH@icloud.com`, ` n2ych@iCloud.com `), so that a working "reset password" email is always followed by a working
sign-in.

## The Process

1. Every login is stored stripped and lower-case, on create and on every later write.
2. The sign-in lookup normalizes what was typed the same way (strips and lower-cases it) before core compares it with `=`. Core's exact
   comparison is kept, and every other override in the lookup chain (for example website scoping) and the index on `login` still apply.
   Password reset already matched the address case-insensitively, which is how a member used to get a working reset mail and then a
   sign-in that failed. (`_get_login_domain`, see the first entry below).

## Verification

`hams_base/tests/test_login_case_insensitive.py`.

## Interface

- `[@ANCHOR: hams_base:res_users_normalize_login]` `ResUsers._normalize_login(login)` (a static method) returns the string stripped of surrounding whitespace and lower-cased; anything that is not a string (`None`, `False`, a number) is returned unchanged. It does no database work and cannot raise. Used by the sign-in lookup, `create` and `write`. **Tests:** `TestLoginCaseInsensitive.test_normalize_login_strips_and_lowercases_a_string_and_passes_anything_else_through`.
- `[@ANCHOR: hams_base:res_users_create]` `ResUsers.create(vals_list)` replaces the `login` of every values dict that has one with its normalized form before core creates the users, so a login is never stored in mixed case or with spaces. It edits the caller's dicts in place and adds no other behaviour. **Tests:** `TestLoginCaseInsensitive.test_create_and_write_store_lowercase_login`.

## Entries

### `ResUsers._get_login_domain(self, login)` [@ANCHOR: hams_base:login_case_insensitive]

An `@api.model` override in `hams_base/models/res_users.py` of the method core uses to build the search domain for the
sign-in lookup. `login` is whatever the member typed (a string, normally). It returns `super()._get_login_domain(
self._normalize_login(login))`: the parent's domain, built from the stripped, lower-cased text, so core's exact `=`
comparison now matches the lower-case stored login whatever capitalisation the keyboard produced. Delegating to `super`
keeps the other overrides in the chain (website scoping, for instance) and the btree index on `login`, which a
`=ilike` search would lose. It does no database access itself and cannot raise for a string; a non-string passes through
`_normalize_login` unchanged. Together with `create` and `write` normalizing stored logins, this makes sign-in
case-insensitive without a data migration for logins that are already lower-case; older mixed-case logins need a one-time
normalization (unknown whether one was run on production; to-do). Test:
`TestLoginCaseInsensitive` in `hams_base/tests/test_login_case_insensitive.py`.
