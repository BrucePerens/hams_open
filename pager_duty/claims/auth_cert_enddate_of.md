---
anchor: pager_duty:auth_cert_enddate_of
code_hash: sha256:d1c16786e04360620bf8ce38fd0144cd67f5382329f022c0cfde5d9a6e750f0c
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: check_auth_cert_expiry.py `_enddate_of` (`pager_duty/daemon/check_auth_cert_expiry.py`)

Read 2026-11-11, whole function. Tier 2: Reads an openssl subprocess; no secret, writes nothing.

1. THE function SHALL run `openssl x509 -noout -enddate` with the given extra `args` (`-in <path>` for a file, none for PEM text passed on stdin), capturing output, with a 15-second timeout.
2. WHEN openssl exits non-zero THE function SHALL raise `ValueError` whose text is `openssl could not read the certificate: ` followed by the first 80 characters of the stripped stderr.
3. WHEN openssl exits zero THE function SHALL return `parse_enddate` of its stdout, a timezone-aware UTC datetime, and a stdout `parse_enddate` cannot read raises `ValueError` from that function.
4. A timeout (`subprocess.TimeoutExpired`) and a missing openssl (`OSError`) are not caught and propagate to the caller. HOLDS.
