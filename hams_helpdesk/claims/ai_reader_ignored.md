---
anchor: hams_helpdesk:ai_reader_ignored
code_hash: sha256:c3c70eddce535b3abfb4887051a9fba7e2d6da43bcc77d254250255578fe883a
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ai_reader_rule._ignored`

THE function SHALL return True when at least one of the given 1-based line numbers is within `1..len(lines)` and the text of that line contains the tag string `ticket-ai-ignore:`. A number of 0, a negative number or one past the end is skipped, never an error. With no line numbers it returns False. HOLDS. Read 2026-11-11 against the code.
