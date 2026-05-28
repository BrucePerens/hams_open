# Issues Encountered During Testing

1. **Incorrect script name in docs**: The instructions mention `tools/test_runner` or `tools/test_runner.py`, but the actual script in the repository is `tools/test.py`.
2. **Provisioning failure**: When attempting to provision the environment using `IN_JULES_VM=1 python3 tools/test.py --provision-jules -u user_websites`, the following error occurred:
   ```
   [*] Configuring local PostgreSQL...
   ❌ ERROR: PostgreSQL initdb not found.
   ```
3. **Test execution failure**: Standard tests could not run because the required PostgreSQL database failed to initialize due to the aforementioned `initdb not found` error.
