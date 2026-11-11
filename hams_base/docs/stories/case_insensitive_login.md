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
   sign-in that failed. `[@ANCHOR: hams_base:login_case_insensitive]`

## Verification

`hams_base/tests/test_login_case_insensitive.py`.

## Interface

- `[@ANCHOR: hams_base:res_users_normalize_login]` `ResUsers._normalize_login(login)` (a static method) returns the string stripped of surrounding whitespace and lower-cased; anything that is not a string (`None`, `False`, a number) is returned unchanged. It does no database work and cannot raise. Used by the sign-in lookup, `create` and `write`. **Tests:** `TestLoginCaseInsensitive.test_normalize_login_strips_and_lowercases_a_string_and_passes_anything_else_through`.
- `[@ANCHOR: hams_base:res_users_create]` `ResUsers.create(vals_list)` replaces the `login` of every values dict that has one with its normalized form before core creates the users, so a login is never stored in mixed case or with spaces. It edits the caller's dicts in place and adds no other behaviour. **Tests:** `TestLoginCaseInsensitive.test_create_and_write_store_lowercase_login`.
