# Journey: Securing Configuration Parameters `[@ANCHOR: journey_securing_configuration]`

This journey describes how a new configuration parameter is safely integrated into the Zero-Sudo architecture.

## 1. Requirement Identification
A module needs to store a setting, for example, `my_module.api_endpoint`, and needs to access it in a context where `.sudo()` is not allowed.

## 2. Whitelisting
The developer must modify `zero_sudo/models/security_utils.py` to add `my_module.api_endpoint` to the list returned by the `_get_param_whitelist` method. This automatically whitelists it for both `_get_system_param` and `_set_system_param` functions `[@ANCHOR: get_system_param]`, `[@ANCHOR: set_system_param]`.

## 3. Secure Access
Now, anywhere in the codebase, the parameter can be retrieved or set safely:
```python
endpoint = self.env['zero_sudo.security.utils']._get_system_param('my_module.api_endpoint')
self.env['zero_sudo.security.utils']._set_system_param('my_module.api_endpoint', '[https://new-api.example.com](https://new-api.example.com)')
```

## 4. Protection against Exfiltration
If an attacker attempts to use a template injection to call `_get_system_param('database.secret')`, the function will raise an `AccessError` because `database.secret` is not in the whitelist and contains the banned substring `secret`.
