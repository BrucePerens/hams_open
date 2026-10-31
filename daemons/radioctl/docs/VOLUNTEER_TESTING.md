# Volunteer testing

radioctl describes every rig Hamlib lists, but its authors own only a handful of radios. A description stays
`unverified` until someone with the radio confirms it. This page tells you how to do that safely, with no
transmitting. The current list of what is verified is [RIG_SUPPORT.md](RIG_SUPPORT.md).

## Run radioctl-selftest

`radioctl-selftest` ships with the hams.com local relay (the relay's packages and downloads install it next to
the relay: `/usr/bin/radioctl-selftest` on Linux, the relay's install folder on Windows and macOS). To get it
from a radioctl checkout instead: `cd daemons/radioctl && cargo build --release -p radioctld --bin radioctl-selftest`.
It is one program for every rig description, with all descriptions built in; there is nothing else to install.

1. Connect the radio's CAT or USB cable, power it on, and set its CAT baud rate and CI-V/CAT options as the
   radio's manual says. Write down the firmware version from its menu if you can.
2. Close every other program that uses the serial port (logger, WSJT-X, `rigctld`).
3. Run `radioctl-selftest`. It finds the radios, prints a table, tests each one in turn, writes a report file per
   radio and uploads each report to hams.com.

## What the self-test does and does not do

- It sends only read commands: identify, frequency, mode, transmit-state read, and the meters and levels the
  description claims. The list is fixed in `crates/radioctld/src/selftest.rs` (`is_read_only`); any other
  operation is refused by the tool itself. It runs only the checks the description claims, so it needs no
  per-rig code.
- It never transmits. It has no PTT, Morse or set command and never opens the transmit gate.
- **It uploads.** At the end of every run it sends one report per radio to `https://hams.com/api/rig-reports`,
  with no question asked (`--no-upload` turns it off). What is sent is exactly the JSON in
  [RIG_REPORT_UPLOAD.md](RIG_REPORT_UPLOAD.md): the check results, the bytes exchanged with the radio, the
  radioctl version, your OS and CPU type, and the name and email you gave. No host name, user name, file path,
  IP address or USB serial number is sent. The local report file is always written first, and the program prints
  what it is about to send and where the local copy is. If the upload fails (offline, an error, 10 seconds
  with no answer) it says so in one line and the test result is unchanged.
- The first time you run it at a terminal it asks for your **name** and then your **email**, so hams.com can
  contact you about your rig; they are sent with the report. Press Enter to skip either. The answers are saved
  in your user configuration folder (`~/.config/radioctl/selftest-contact.json`, readable only by you) and you
  are not asked again. For scripts use `--name`, `--email` or the environment variables
  `RADIOCTL_SELFTEST_NAME` and `RADIOCTL_SELFTEST_EMAIL`; a run that is not at a terminal never waits for input.
- It records every byte exchanged with the radio, both directions, in the report. This is what lets a maintainer
  check the description without your radio.

After a **successful** upload it thanks you, once per radio (and a short closing line when several were tested):

> Thank You! You have verified the operation of Icom IC-7300 1.2. Please feel free to re-run this program any time and
> with new radios. We may contact you again if we see bugs and want to resolve them.

The rig is the maker and model; the version is the firmware read from the radio and is left out when the radio did not
give one. If a check did not pass it says instead "Thank You! Your report on Icom IC-7300 1.2 is in. 1 of 12 checks did
not pass. That is a problem in our software, not something you did wrong, and we will look into it." followed by the same two sentences (checks the description does not support are not
counted). With `--no-upload`, or when the upload fails, there is no thank-you, only the one-line status.

## The VFO knob check

For a radio whose description claims that it reports its own changes (Icom CI-V transceive, Elecraft/Kenwood/Yaesu
auto-information, Flex subscriptions), and only at a terminal, the last check asks you to turn the VFO knob:

> Please turn the VFO knob a little, in either direction, now (waiting up to 15 seconds; press Enter to skip)

The check sends nothing to the radio. It listens to the radio's own status events and counts only changes the radio
reported without being asked (not the answers to our polls or commands), and reports the frequency change it saw.
Press Enter to skip it. If nothing arrives it says: "No unsolicited update was seen. Some radios need this turned on
in the menu (for example the KX3's auto-info/transceive setting, or CI-V Transceive on an Icom); if yours has it,
enable it and run this again. If it is on, this is a problem in our software." The program never turns the setting
on by itself. Without a terminal the check is skipped (`skipped: no terminal`) and does not count against the radio.
In the upload it is an ordinary check named `unsolicited status (turn the VFO knob)`.

## Updating

The copy that ships with the local relay checks at the start of each run whether a newer version has been
released, using the relay's own release channel and signing (the same `latest_info` and signed bundle the relay
updates from; there is no separate trust scheme). It never updates silently:

- At a terminal it asks `A newer radioctl-selftest (X.Y.Z) is available; update now? [y/N]`. On `y` it downloads the
  signed bundle for your OS and CPU, checks its sha256 and its signature against the relay's release key, replaces
  itself (the old program is kept next to it as `radioctl-selftest.prev`) and starts again with the same arguments.
- Without a terminal it only prints the notice; `--update` installs without asking. `--no-update-check` skips the check.
- Offline, a slow server (5 seconds), a failed or unverifiable download: one line, the existing program is left
  untouched, and the test carries on.
- A copy you built from source has no update channel and does not check. As with the relay, there is no version
  floor: a newer signed release is installed, and a bad release is answered by a newer one or the signed revocation list.

## Discovery (the default)

With no options the program looks for radios and prints a table first: each USB serial port with its USB ids and
product string, and what was decided.

- A port whose own text says it is not a radio (a GPS receiver, an Arduino, a programmer) is skipped and never opened.
- A port whose product string names exactly one description is tested with it.
- A port whose product string fits several descriptions (`TS-590S` and `TS-590SG`) is asked with the
  descriptions' own read-only identify commands, at the rates the baud probe tries, and chosen by the answer.
  If that does not leave exactly one, the table says `ambiguous: candidates ...` and the port is skipped.
- A port with no such evidence (most USB cables only identify their chip, not the radio) is `unidentified`, is
  never written to, and is not tested. Name it yourself with `--description` and `--port`.
- FlexRadio network radios that announce themselves are listed (the program only listens). Testing network
  radios is not supported yet.

One radio is tested at a time. Each gets its own report (`radioctl-selftest-ID.txt`, in `--out-dir` or the
current folder) and its own upload, and a run summary `radioctl-selftest-summary.txt` is written.

## Choosing a radio yourself

```
radioctl-selftest --list
radioctl-selftest --description icom-ic-7300 --port /dev/ttyUSB0 --baud 115200
radioctl-selftest --description yaesu-ft-991a --port usb:10c4:ea60 --baud auto
radioctl-selftest icom-ic-7300 /dev/ttyUSB0 115200
```

- `--list` prints the ports found, the USB ids, and every description (the built-in set, or `--rigs-dir DIR`, or
  `$RADIOCTL_RIGS`, in that order of precedence).
- `--description ID` is a file name in `rigs/` without `.toml`. A neighbouring model is not a test of yours: do not
  run the FT-817 description on an FT-818 unless asked.
- `--port` is a serial path, `usb:VID:PID[:SERIAL]` (hexadecimal), or `virtual` (an in-memory radio, to try the
  tool without a radio). Naming only one of the two uses the only port, or the port's own product string.
- `--baud N` sets the rate (default 19200). `--baud auto` tries the usual rates with the description's own
  frequency read and uses the first that answers; the rates tried are in the report.
- `--no-discover` turns discovery off (then `--description` and `--port` are required). `--no-upload` writes the
  local report only. `--server URL` uploads somewhere else (https, or http to this computer; for testing).
  `--out FILE` names the report of a single radio; `--out-dir DIR` the folder for all of them.
- `--unmanaged` leaves the DTR and RTS lines alone (needed for pseudo-terminals).
- Exit status: 0 if no check failed, 1 if a check failed or a radio could not be opened, 2 for a usage error. A
  failed upload never changes it.

Each check prints `Pass`, `Fail` or `NotApplicable`. A check that did not pass is a problem in radioctl or in the rig
description, never something you did wrong, and it is a useful result: the report says which check and what was sent
and received. (It could also be that the radio is off or the port differs; if so, run again.)

## What to add to your report

The upload already carries the results and the bytes. To help us check the radio, also write to us (reply to the
contact you were given, or file an issue) with:

- the radio's exact model and firmware version, and any option boards or accessories on the CAT line;
- the cable or interface (the radio's own USB, a CP210x/FTDI cable, a level shifter) and the radio's CAT settings;
- what you saw on the radio itself (the frequency and mode on its display when you ran the test).

The raw bytes can include a radio serial number or a callsign set in the radio; the report file is yours to read
before you share it anywhere else.

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

1. Read the uploaded report (or the issue) and the whole report. Confirm the model, the description id and that the radio was in a sane state.
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
