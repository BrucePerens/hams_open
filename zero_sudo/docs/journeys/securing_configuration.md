<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Journey: Securing Configuration Parameters `[@ANCHOR: zero_sudo:COMM_journey_securing_configuration]`

This journey describes how a new configuration parameter is safely integrated into the Zero-Sudo architecture.

## 1. Requirement Identification
A module needs to store a setting, for example, `my_module.api_endpoint`, and needs to access it in a context where `.sudo()` is not allowed.

## 2. Whitelisting
The developer must inherit `zero_sudo.security.utils` and override `_get_param_whitelist` to include `my_module.api_endpoint`. This automatically whitelists it for both `_get_system_param` and `_set_system_param` functions `[@ANCHOR: zero_sudo:COMM_get_system_param]`, `[@ANCHOR: zero_sudo:COMM_set_system_param]`.

Note: the [module README](../../README.md) and [parameter_whitelisting.md](../stories/parameter_whitelisting.md) describe two separate whitelists, a read list (`_get_param_read_whitelist()`) and a write list (`_get_param_write_whitelist()`); a key must be on the list for the operation it is used for.

## 3. Secure Access
Now, anywhere in the codebase, the parameter can be retrieved or set safely:
```python
endpoint = self.env['zero_sudo.security.utils']._get_system_param('my_module.api_endpoint')
self.env['zero_sudo.security.utils']._set_system_param('my_module.api_endpoint', 'https://new-api.example.com')
```

## 4. Protection against Exfiltration
If an attacker attempts to use a template injection to call `_get_system_param('database.secret')`, the function will raise an `AccessError` because `database.secret` is not in the whitelist and contains the banned substring `secret`. (The whitelist is what decides: a read of a key that is not on it is always denied, and a matching substring only changes the error message; the substring check does not apply to whitelisted keys. See the [module README](../../README.md), "Mechanical Secret Block".)
