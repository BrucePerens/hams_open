# Story: Secure Path Validation [@ANCHOR: backup_management:COMM_story_secure_path_validation]

This story describes the security measures taken to ensure backup operations cannot be used to access unauthorized system files.

## Background
Since the backup module interacts with the filesystem via shell commands, it must prevent path traversal attacks or unauthorized access to sensitive directories.

## The Process
1. **Input Validation**: Whenever a path is entered (target path, restore script path), it must be checked for directory traversal attempts (e.g., `..`).
2. **Mandatory Base Directory**: Paths must be validated against a mandatory base directory using `os.path.realpath` to prevent path traversal (CWE-22). Blacklists are strictly forbidden.
3. **Enforcement**: The validation `[@ANCHOR: backup_management:COMM_backup_path_validation]` happens at the ORM level during `create` and `write` operations.

## Verification
Security path validation is verified by `[@ANCHOR: backup_management:COMM_test_backup_security]`.
