# Volunteer testing

radioctl describes every rig Hamlib lists, but its authors own only a handful of radios. A description stays
`unverified` until someone with the radio confirms it. This page tells you how to do that safely, with no
transmitting, no upload and no account other than the GitHub issue you choose to file. The current list of what is
verified is [RIG_SUPPORT.md](RIG_SUPPORT.md).

## What the self-test does and does not do

- It sends only read commands: identify, frequency, mode, transmit-state read, and the meters and levels the
  description claims. The list is fixed in `crates/radioctld/src/selftest.rs` (`is_read_only`); any other
  operation is refused by the tool itself.
- It never transmits. It has no PTT, Morse or set command, never opens the transmit gate, and makes no network
  connection. Nothing is uploaded and there is no telemetry. The report is a file on your disk.
- It records every byte exchanged with the radio, both directions, in the report. This is what lets a maintainer
  check the description without your radio.

## Before you start

1. Build radioctl from a checkout: `cd daemons/radioctl && cargo build --example rig_selftest`.
2. Connect the radio's CAT or USB cable, power it on, and set its CAT baud rate and CI-V/CAT options as the radio's
   manual says. If you can, write down the radio's firmware version from its menu.
3. Close every other program that uses the serial port (logger, WSJT-X, `rigctld`).
4. Run `cargo run --example rig_selftest -- --list`. It prints the serial ports it finds, the USB ids and the
   descriptions that exist. USB ids only identify the cable's chip, so the list does not choose for you: it names a
   description only when the device's own product string contains the model, and says AMBIGUOUS when two fit.
   If your model has no description, see [RIG_SUPPORT.md](RIG_SUPPORT.md) and file an issue to ask for it.

## Run the read-only self-test

```
cargo run --example rig_selftest -- RIG_ID TARGET [BAUD]
cargo run --example rig_selftest -- icom-ic-7300 /dev/ttyUSB0 115200
cargo run --example rig_selftest -- --out my-report.txt yaesu-ft-991a usb:10c4:ea60 38400
```

- `RIG_ID` is a file name in `rigs/` without `.toml`. Pick the one that names your radio. A neighbouring model is
  not a test of yours: do not run the FT-817 description on an FT-818 unless asked.
- `TARGET` is a serial path or `usb:VID:PID[:SERIAL]` (hexadecimal). `virtual` runs against an in-memory radio to
  try the tool without a radio.
- `BAUD` defaults to 19200; use the rate set in the radio.
- The report is `rig_selftest-RIG_ID.txt` unless you pass `--out`.

Each check prints `Pass`, `Fail` or `NotApplicable`. A `Fail` or a timeout is a useful result: do not fix the
settings until after you have saved the report, then file it.

## What to include in the report

Attach the whole report file (it already has the tool version, your system, the description and its facts, the
target, every check and the raw bytes) and add in the issue text:

- the radio's exact model and firmware version, and any option boards or accessories on the CAT line;
- the cable or interface (the radio's own USB, a CP210x/FTDI cable, a level shifter) and the radio's CAT settings;
- the radioctl commit (`git rev-parse HEAD`) and whether you changed anything in `rigs/`;
- what you saw on the radio itself (the frequency and mode on its display when you ran the test).

Before posting, open the file and remove anything private. The tool removes a USB serial number from the target, but
the raw bytes can include a radio serial number or a callsign set in the radio. Replace such values with `XXXX`,
and say in the issue that you did.

## How to file it

There is no upload command. Open a new issue in the radioctl repository on GitHub, titled
`rig test: MAKER MODEL (description-id): result`, paste or attach the report file, and add the items above. The
maintainers read it; no one else is contacted. If you have no GitHub account, send the file to the maintainers
by the contact in the repository README.

## Transmit tests: a separate, manual procedure

The self-test never transmits, so a pass says nothing about PTT or CW. The commands exist in the descriptions for
rigs that declare them, but for any rig that is not `hardware-verified` they are **encoded and unverified**. The transmit gate is
closed by default and stays closed unless you open it deliberately.

Only do this with a **dummy load** (or a tuned antenna you have checked) and with power turned down to 5 W or less:

1. Complete the read-only self-test and file it first.
2. Connect the dummy load. Never test into an open or mismatched antenna connector.
3. Start `radioctld` with a config for this rig that sets `transmit = true` (see `radioctld.example.toml`), with
   the rig's power setting at its lowest.
4. With `rigctl -m 2 -r 127.0.0.1:4532`, send `T 1`, then at once `T 0`, and watch that the radio keys and unkeys.
   Repeat with CW only if the description claims it, and send a single short character.
5. If the radio stays keyed, switch it off and unplug the cable. Report that as a bug with the wire log
   (`bench_console --wire` records one).

Report transmit results in the same issue, saying `transmit tested` and what happened. Maintainers treat them as
a separate item from the read-only checks.

## How a maintainer promotes a description

Levels, from lowest: `unverified` (no check) < `manual-checked` (compared with a vendor manual) <
`transcript-checked` (replayed against a recorded transcript) < `hardware-verified` (tested on a real radio).
The level is stored per fact in `rigs/<id>.toml`, in the fact's `verified` field; there is no other copy.

1. Read the issue and the whole report. Confirm the model, the description id and that the radio was in a sane state.
2. `transcript-checked`: scrub the wire log (serial numbers, callsigns), turn it into a `[[step]]` transcript under
   `rigs/transcripts/` with `origin = "capture"`, and set the facts it covers to
   `verified = { state = "transcript-checked", reference = "<transcript path>" }`.
3. `hardware-verified`: for each fact the report shows working, set
   `verified = { state = "hardware-verified", radio = "...", firmware = "...", date = "YYYY-MM-DD", tested = "..." }`
   with exactly what was tested. Do not mark a fact the report did not exercise, and mark PTT and CW only if the
   transmit procedure was run and the result was good.
4. Add a `[[report]]` to `rigs/reports/verified.toml` (rig, radio, firmware, date, reporter, issue URL, transmit_tested).
   This is the only place the reporter is named, and only as they wish.
5. Fix the description for anything that failed and say so in the issue.
6. Run `python3 tools/rig_support_matrix.py` to regenerate [RIG_SUPPORT.md](RIG_SUPPORT.md). The test
   `tools/test_rig_support_matrix.py` fails if the file is stale or if the facts and `verified.toml` disagree.

## Questions people ask

- *Can I run it with the radio's own control program open?* No. Two programs on one serial port garble both.
- *My radio is not listed.* Ask for it in an issue; radioctl implements every Hamlib model in the order in
  [RIG_COVERAGE_PLAN.md](RIG_COVERAGE_PLAN.md).
- *Does a pass mean I can use it at my station?* It means the read commands worked. Everything else is still
  unverified until the matrix says otherwise.
