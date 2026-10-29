---
title: Verification levels and verified tags
summary: What the verified line of a page means and how much to trust a function from it.
see_also: [concepts/etiquette]
verified: code only, 2026-10-29
---

# Verification levels and verified tags

Every page has a `verified` line in its front matter (returned by `bench_manual_read` as `verified`). It says what was checked and when, so that you do not take a documented function for a tested one.

| Form of the line | Meaning |
|---|---|
| a description and a date (`measured on the unit (VNWA 3E, firmware v5.22), 2026-10-29`) | the function was run on the named real hardware on that date and behaved as the page says |
| `fixtures only` | tested only against recordings (a copy of sysfs, a saved PipeWire graph); the logic is exercised, the hardware is not |
| `code only` | written from the source, the protocol notes and the README; not run on hardware for this page |
| `in progress` | a placeholder, being written; do not rely on it |
| `see <page>` | the page inherits the verification statement of the named page |
| `n/a` | navigation or changelog; nothing to verify |

Rules of thumb:

* A function marked with a date is trusted for the conditions described (the range, the cabling). Outside them, treat it as unverified.
* `code only` pages are correct about arguments, ranges and refusals (the argument tables come from the schema), but the numbers about hardware behaviour are second-hand. Say so in your own report when you rely on them.
* Results carry the same honesty: a VNA sweep is raw unless calibrated, SDR levels are dBFS and not power, the VNA frequency meter's sign and absolute accuracy are not verified against a standard.
* The tests of the repository require every page to have a `verified` line that starts with `n/a`, `fixtures only`, `code only`, `in progress` or `see`, or contains a date.
