## Review Report: zero_sudo — Architecture_and_Security_Reviewer

**Reviewer Role:** Architecture_and_Security_Reviewer
**Module Path:** `/home/bruce/workspace/hams_open/zero_sudo`
**Files Reviewed:** 9
**Total Findings:** 1 ERROR, 3 WARNINGS

### Summary

The architecture and test performance paradigms are largely compliant, specifically regarding zero-sudo and dynamic database query caching. However, there is an environment variable configuration lapse in the test dummy daemon and a minor security issue where an SSL private key is given world-readable permissions during test setup.

### Findings

| # | Severity | File | Line | Issue Description | TargetContent | ReplacementContent |
|---|----------|------|------|-------------------|---------------|--------------------|
| 1 | WARNING | `zero_sudo/tests/common.py` | 772 | Private SSL key is given world-readable (`644`) permissions. Private keys must be `600`. | `subprocess.run(["chmod", "644", key_path], check=False, shell=False)` | `subprocess.run(["chmod", "600", key_path], check=False, shell=False)` |
| 2 | WARNING | `zero_sudo/tests/common.py` | 306 | `open()` without specifying encoding may result in encoding errors on different platforms. | `with open(filepath, "w") as f:` | `with open(filepath, "w", encoding="utf-8") as f:` |
| 3 | WARNING | `zero_sudo/tests/common.py` | 323 | `write_text()` without specifying encoding may result in encoding errors. | `host_path.write_text(content)` | `host_path.write_text(content, encoding="utf-8")` |
| 4 | ERROR | `zero_sudo/tests/dummy_daemon.py` | 20 | Hardcoding "0.0.0.0" violates ADR 0079. Must use a two-argument environment variable fallback to "localhost". | `if __name__ == "__main__":<br>    with socketserver.TCPServer(("0.0.0.0", PORT), Handler) as httpd:` | `import os<br><br>if __name__ == "__main__":<br>    host = os.environ.get("DUMMY_DAEMON_HOST", "localhost")<br>    with socketserver.TCPServer((host, PORT), Handler) as httpd:` |

*(Note: Newlines in Target/Replacement content in markdown tables can be tricky; I used `<br>` for dummy_daemon.py display purposes. In the source file, they should be actual newlines.)*

### Areas Reviewed With No Issues

- `zero_sudo/static/src/components/security_dashboard/security_dashboard.xml` — UI escaping prevents XSS.
- `zero_sudo/static/src/js/tour_failure_dump.js` — Watchdog hooks handle UI tracking correctly.
- `zero_sudo/static/src/js/tour_utils.js` — Secure JS utilities without eval issues.
- `zero_sudo/static/src/tours/zero_sudo_tour.js` — Standard JS tour implementation.
- `zero_sudo/tests/__init__.py` — DB Cursor mocking safely catches serialization deadlocks.
- `zero_sudo/tests/real_transaction.py` — Database teardown avoids N+1 queries using `UNION ALL` procedures and respects zero-sudo constraints.
- `zero_sudo/tests/test_controllers.py` — Safe param execution and clear verification of interceptors.
