---
anchor: hams_helpdesk:daemon_main
code_hash: sha256:674c1cabfb381597b0a4280d202325e83377eb954ebe83a8099b50f5e06e55f4
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `exception_reporter.main`

THE function SHALL return 2 and print a usage line to stderr unless argv is exactly `["report-unit", <unit>]`. For that shape it calls `report_unit(unit)` once and returns 0 after printing the JSON answer, or returns 1 after printing only the `ReportError` text to stderr. With `argv=None` it reads `sys.argv[1:]`. HOLDS. Read 2026-11-11 against the code.
