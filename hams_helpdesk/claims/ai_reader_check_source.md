---
anchor: hams_helpdesk:ai_reader_check_source
code_hash: sha256:baf0027467afa11f14aac29a011639bfc336bce12deed39ce00ec17e5f62f486
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ai_reader_rule.check_source`

THE function SHALL return `[]` for source that does not parse. Otherwise it returns `(line, message)` for (1) each `mcp_*` method of a class whose `_name` or `_inherit` is a ticket-like model that loads one of `TEXT_FIELDS` as an attribute and whose subtree has no `SAFE_MARKERS` marker, and (2) only in a file that contains one of `AI_FILE_MARKERS`, each `execute`/`execute_kw`/`call` of a read method on a ticket-like model whose field list is unknown, empty or holds a text field, inside a function without a safe marker. A `# ticket-ai-ignore:` tag on the def line, the line above it (shape 1) or the call line (shape 2) skips the finding. It never raises and never reads a file. HOLDS. Read 2026-11-11 against the code.
