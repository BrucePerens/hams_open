# Story: Parameter Whitelisting `[@ANCHOR: COMM_story_parameter_whitelisting]`

This story describes how sensitive system parameters are protected from unauthorized access or exfiltration.

## Background
System parameters (`ir.config_parameter`) often contain configuration that, if leaked, could compromise the system. Server-Side Template Injection (SSTI) is a common vector for exfiltrating these parameters.

## The Process
1. **Access Request**: A module needs to retrieve a system parameter using `_get_system_param` `[@ANCHOR: COMM_get_system_param]`, or set one using `_set_system_param` `[@ANCHOR: COMM_set_system_param]`.
2. **Whitelist Check**: The function checks if the requested key is in the list returned by `_get_param_whitelist`.
3. **Banned Substring Check**: Even for non-whitelisted keys (if the policy allows), it checks for substrings like `secret`, `key`, `password`, etc.
4. **Restricted Retrieval**: If the key passes all checks, it is retrieved using a dedicated micro-privilege service account (`zero_sudo.config_service_internal`) and returned.

## Developer Requirement
If a new, safe parameter needs to be accessible via this utility, the developer MUST add it to the list returned by the `_get_param_whitelist` method in `zero_sudo/models/security_utils.py`.

## Cryptographic Secrets
Cryptographic secrets are strictly forbidden from entering the parameter whitelist to prevent SSTI. To retrieve the system's root cryptographic key, developers must use the `_get_crypto_secret` utility `[@ANCHOR: COMM_get_crypto_secret]`. This function reads from environment variables, local files, or Odoo's base configuration without evaluating the database's `ir.config_parameter` table.
