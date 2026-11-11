---
anchor: hams_helpdesk:daemon_reporting_failures_exit
code_hash: sha256:0cbefb929423b9aefaad10b73999328084b2ad0a8a8434d3f9c24cdc3f6145c6
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `exception_reporter.reporting_failures.__exit__`

THE method SHALL call `report_exception(self.daemon_name, exc, **self.options)` exactly when `exc` is an instance of `Exception`, and SHALL always return False, so the original exception propagates unchanged. A `SystemExit`, `KeyboardInterrupt` or no exception reports nothing. HOLDS. Read 2026-11-11 against the code.
