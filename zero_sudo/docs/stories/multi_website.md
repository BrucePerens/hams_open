# Story: Multi-Website Awareness `[@ANCHOR: story_multi_website]`

This story describes how the `zero_sudo` security core handles multi-website environments.

## Web Login Isolation
The `is_service_account` check in `web_login` `[@ANCHOR: web_login_interceptor_check]` is a global security mandate. It operates at the session level, ensuring that any user flagged as a service account is immediately logged out if they attempt an interactive web login, regardless of which website they are accessing.

## Documentation Bootstrap
The documentation installer `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]` is designed to be website-agnostic. It injects documentation into the central knowledge base, making it available across the entire Odoo instance. If the underlying documentation model (`knowledge.article` or `knowledge.article`) supports website-level isolation, `zero_sudo` respects the platform's default visibility settings.

## System Parameters
System parameters managed via `zero_sudo.security.utils` `[@ANCHOR: get_system_param]` are typically global configurations (`ir.config_parameter`). The whitelist ensures that only safe, intended parameters are accessible in a multi-tenant or multi-website context.

## Global Models
Certain models in `zero_sudo` are logically global and do not track `website_id` or `company_id`:
- **KV Store (`zero_sudo.kv`) `[@ANCHOR: COMM_zero_sudo_kv_global]`**: Stores platform-wide technical state.
- **Noisy Tables (`zero_sudo.noisy_table`) `[@ANCHOR: COMM_zero_sudo_noisy_table_global]`**: Lists PostgreSQL tables ignored globally for leak detection.
