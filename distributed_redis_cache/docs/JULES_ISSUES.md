# Jules Issues - distributed_redis_cache (Session 2026-05-30)

## Environment Verification
- **Issue: Missing Python Dependencies**: The environment was missing `asyncpg` and `websocket-client` in the system Python.
- **Issue: Daemon PYTHONPATH Isolation**: The `zero_sudo.daemon.utils` module hardcodes `PYTHONPATH` to `/usr/lib/python3/dist-packages`, which prevents it from finding packages installed via `pip install --user` or in `.local`. This caused the `cache_manager.py` daemon to fail with `ModuleNotFoundError: No module named 'asyncpg'` during tests.
- **Resolution**: Manually installed missing dependencies (`asyncpg`, `websocket-client`, `ntplib`, `pymysql`, `ldap3`) using `sudo apt-get` or `pip --break-system-packages` to ensure they are visible to both Odoo and standalone daemons.

## AI Hallucination & Laziness Audit
- **Audit Outcome: test_enable bypasses**: Audited `redis_cache.py` and `distributed_cache_config.py`. Found that `tools.config.get("test_enable")` is used strictly to *safeguard* Redis connectivity during standard test transactions to prevent cross-test data poisoning. It correctly yields to a system parameter for explicit integration testing. No lazy "skip test" hallucinations were found.
- **Audit Outcome: ir_http.py background threads**: Verified that `ir_http.py` utilizes the architecturally mandated `concurrent.futures.ThreadPoolExecutor` instead of `threading.Thread`. The listener lifecycle is correctly managed via `atexit` and thread-safe locking.
- **Catch-all Exceptions**: Identified several `except Exception:` blocks in `daemons/cache_manager.py`. While appropriate for a top-level daemon loop to maintain resilience, they were missing the required `# audit-ignore-catch-all` tags.
- **Resolution**: Added `# audit-ignore-catch-all` to all verified daemon catch-all blocks.

### Proposed Linter Rules for `check_burn_list.py`
To combat AI-generated shortcuts and laziness, the following regex rules are proposed for `check_burn_list.py`:

1. **Tautological Assertions**: Detects tests that assert constants.
   - `re.compile(r"self\.assert(True|False|Equal|IsNone)\s*\(\s*(1\s*==\s*1|True\s*==\s*True|False\s*==\s*False|None\s*is\s*None)\s*\)")`
   - *Message*: "AI LAZINESS: Tautological assertion detected (e.g. 1 == 1). Tests must verify actual logic."

2. **Inline Dependency Installation**: Prevents soft fallbacks that install OS or Python packages at runtime.
   - `re.compile(r"subprocess\.(run|call|Popen|check_output)\s*\(\s*\[\s*['\"](pip|apt|apt-get)['\"]")`
   - *Message*: "AI HALLUCINATION: Inline dependency installation detected. Software must FAIL FAST if dependencies are missing. Declare them in __manifest__.py or requirements.txt."

3. **Lazy Method Bypassing**: Detects `hasattr` checks on `self` in tests used to bypass missing methods.
   - `re.compile(r"if\s+hasattr\s*\(\s*self\s*,\s*['\"][^'\"]+['\"]\s*\):")`
   - *Message*: "AI LAZINESS: Conditional logic using hasattr(self, ...) in tests detected. Ensure the environment is correctly provisioned instead of bypassing."

## Security & Multi-tenant Awareness
- **ADR-0083 Violation**: Found a critical multi-tenant context management violation in `redis_cache.py` where `allowed_company_ids` was manually read from `self.env.context`.
- **Resolution**: Refactored to use `self.env.company.id`, which is the architecturally mandated method for identifying the current company context in Odoo 19.

## Micro-Privilege & Zero-Sudo
- **Compliance**: Module is fully Zero-Sudo compliant. All operations execute with minimum privilege using service user IDs or specific privilege groups. No usage of `.sudo()` was detected.
