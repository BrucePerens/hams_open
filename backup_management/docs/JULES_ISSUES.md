# JULES ISSUES - backup_management

## AI Hallucination & Laziness
- Found `hasattr(self, "user_no_group")` in `backup_management/tests/test_backup_security.py`. This is a shortcut to bypass potential setup failures.

## Multi-Tenant Awareness
- Models `backup.config`, `backup.snapshot`, and `backup.job` have `website_id` but are missing `company_id`.
- Security rules use `website_id` but do not account for `company_id`.

## Security
- Review of `_get_executable` shows it returns a path, but it's used in `_publish_to_worker` which eventually sends it to RabbitMQ. Need to ensure the worker doesn't blindly execute anything.
- `kopia_password` and `secret_key` are encrypted at rest but decrypted in memory.
- The use of `os.environ.get("ODOO_BACKUP_CRYPTO_KEY")` is appropriate.

## Documentation
- `README.md` exists but could be more detailed for non-technical users.
- `data/documentation.html` needs to be verified/updated.
