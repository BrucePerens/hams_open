<!-- bench_mcp instrument contribution. Use with: gh pr create --body-file <this file, filled in>. Guide: daemons/bench_mcp/docs/EXTENDING.md. Delete this comment. -->

## Instrument and function

Model and firmware version:
Connection:
What this adds (one instrument or one function):

## New tools

| Tool | Tier | Why this tier | dry_run | confirm |
|---|---|---|---|---|

## Hardware testing

Run on real hardware, by whom, how (unit, firmware, cabling, levels):
Output of the ignored hardware test (scrubbed):
NOT tested on hardware (and so marked `fixtures only` or `code only`):

## Safety

Highest-risk function and its protection (limits, confirm, dry_run, time limit, safe-state tool):
Radiates or touches mains/high voltage? Owner's statement on compliance:
The owner gave permission before the hardware was touched: yes / no

## Provenance

Sources used, by kind (vendor programming manual, own replies, owner's own captures, measurements):
Statement: nothing here is copied from vendor source, firmware, drivers, disassembly or help text, and no vendor SDK is vendored:
Anything unusual about licensing (vendor SDK needed, etc.):

## Scrubbing

Serials, MAC/IP addresses, keys, recordings removed from fixtures, pages and logs: yes / no, what:

## Checks run (paste the result lines)

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test` (includes the checklist and manual tests)
- [ ] a page for every tool; standard instrument pages; README example; `whats-new` line
- [ ] no entry added to any LEGACY table in `src/checklist.rs`
- [ ] the owner approved this text and the fixtures before the PR was opened

## Notes for the reviewer
