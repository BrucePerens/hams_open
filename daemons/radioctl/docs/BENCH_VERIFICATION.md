# Bench verification runbook

Written 2026-11-02. One document for every hardware hand-check that is open across radioctl, bench_mcp and the relay, in the order to
do them on one bench day. Nothing in it has been run: every expected result is what the code and the datasheets predict, and the
first surprise is to be written down, not worked around. The relay-side checks (PTT dead-man, WinKeyer, USB audio, antenna switches,
rotator, SDR transmit, self-update) are in `docs/runbooks/RELAY_BENCH_CHECKLIST.md` in hams_com; they have ids `R1` to `R18` and
`U1` to `U5` and are in the day plan below. The guided runner (`bench_verify.py`, see the end) prints these checks and records their
outcome, but it replaces none of the judgement here.

## 1. What Bruce needs to bring and do

Do before the day (each takes minutes; none needs a transmitter):

1. **Toughpad:** add the `ai` account to `dialout` (`sudo usermod -aG dialout ai`, log in again) or run the tools under `sg dialout`. Without it the
   radio's port (`root:dialout 660`) gives EACCES. The FTDI cable on `/dev/ttyUSB0` there is not the radio.
2. **IC-7300 menus:** photograph the CI-V block (address, baud, USB baud, transceive, USB Echo Back, USB port), Version, and set **USB SEND, USB Keying
   (CW) and USB Keying (RTTY) to OFF** and Echo Back OFF, before the cable goes in (check A1).
3. **Sampler and scope:** read the rating of the 50 dB sampler and the scope input limit; the VNWA's maximum RX input is unverified, so the **scope
   comes first** and the VNWA only after the sampler's output is measured to be well below its limit (A3).
4. **Decide, in chat, and tell the AI:** (a) the FlexRadio 6300 is receive-only until you say otherwise: say if it may transmit into a dummy load
   (C7); (b) confirm or replace the `bench_scope_self_cal` entry in `FULL_WITHOUT_SAFE_STATE` in `bench_mcp/src/checklist.rs` (E4); (c) which of
   the optional items you have (power meter or coupler, K1EL WinKeyer, a Pi with a USB audio adapter, Shelly/Tasmota/Waveshare boards, a rotator
   controller): section 3 marks each.
5. **KX3:** the right cable (it had the wrong one on 2026-10-30), or leave the KX3 for another day (B13).
6. **Do not probe `/dev/ttyUSB0` on the dev box.** It is the SainSmart 16-relay board (CH340, `1a86:7523`); the ATM6301 supply is also a CH340. Every
   command here names `/dev/serial/by-id/...` or `usb:VID:PID`, never a bare `ttyUSBn`.

On the day, Bruce must be physically present for every check marked **Bruce present**: all of phase C, the relay checks that key a carrier,
and anything that unplugs or switches a radio. A hand stays on the radio's power switch (or the dummy load's feed) for those.

## 2. The rules that apply to every check

1. **Transmit only into a dummy load.** Never into an antenna. Look at the load on the antenna port before the gate is opened; the software cannot.
2. **Power at the radio's minimum** (IC-7300: a few watts; target 5 W or less at the load, VOLUNTEER_TESTING.md rule), short keyed bursts, watch the load.
   At 5 W (+37 dBm) the 50 dB sampler gives about -13 dBm. Do not exceed the load's and the sampler's rating, and do not run the load hot.
3. **The gate is opened deliberately and only for the check:** `transmit = true` in the radioctld config used for that check, `max_transmit_s = 5`.
   Every other check uses a config with `transmit = false`. Never edit a running config; start a different one.
4. **Stop at the first surprise** (a stuck PTT, a power reading far above the setting, heat, an unexpected tone, a command the radio rejects in a way
   the description did not predict). Unkey by hand first, then write down the exact lines.
5. **One transmit procedure, reused.** Every keyed check uses the procedure of `VOLUNTEER_TESTING.md` ("Transmit tests") and step 8 of the IC-7300
   plan (`night_shift_todo/medium/radioctl-ic7300-bench-day-plan-8d1f3c52.md`): `radioctld --config <rig>-bench-tx.toml`, keep **one connection open**
   while keyed (a one-shot `rigctl T 1` unkeys at once on disconnect, by design), `python3 dryrun/hold.py 8` or
   `(echo "T 1"; sleep 3; echo "T 0"; sleep 1) | nc 127.0.0.1 4532`. Called **TX-PROC** below.
6. **radioctld holds the serial port exclusively** (TIOCEXCL). Stop it before `rigctl -m 3073` or the selftest on the same port.
7. **Evidence:** for each check keep the console log (`bench_console --log`), the wire capture (`--wire`), the scope screenshot or capture
   (bench_mcp `bench_screenshot`, saved under `BENCH_MCP_OUT`) and a line in the day log (`bench_verify.py` writes it). Scrub serial numbers before
   anything is committed (`wire_to_transcript.py --redact HEX`).
8. **Kill what you start:** `pkill -x radioctld; pkill -x bench_console; pkill -f virtual_radio_pty`, remove `/tmp/ic7300-virtual-pty`.
9. **Upload policy.** `radioctl-selftest` uploads its report automatically (Bruce's decision; `--no-upload` turns it off). The runner's own bench report
   and `verified.toml` proposal are never uploaded. When the runner invokes the selftest as a step it passes `--no-upload` unless `--upload` is given.

### How an outcome is recorded (all checks)

- **Rig report (`BENCH-REPORT-<rig>-<date>.txt`)**: the runner's plain-text report in the format of `radioctl-selftest`'s `render_report`: header lines
  (`description:`, `target:`, `serial identity:`), one line per check (`PASS` / `DID NOT PASS` / `N/A` / `SKIPPED (dry run)`), `summary:`, then the wire log.
- **Fact levels** live in `rigs/<id>.toml`, per fact, in the `verified` field; there is no other copy. Levels from lowest: `unverified` < `manual-checked`
  < `transcript-checked` < `hardware-verified`. A check **unlocks** the facts it exercised: set
  `verified = { state = "hardware-verified", radio = "...", firmware = "...", date = "YYYY-MM-DD", tested = "exactly what ran" }`. Never mark PTT or CW
  unless the TX-PROC ran and was good.
- **`rigs/reports/verified.toml`**: one `[[report]]` per radio and test day (rig, radio, firmware, date, reporter, issue, transmit_tested). The runner writes
  `verified.toml.proposed` and a list of facts to flip; a maintainer edits the real files, then `python3 tools/rig_support_matrix.py` (the test fails if facts
  and `verified.toml` disagree) and `cargo test -p radioctl-protocols --test rig_files` for transcripts.
- **Transcripts:** `wire_to_transcript.py --rig ID --wire WIRE --log LOG [--redact HEX]...` then copy to `rigs/transcripts/`, which gives `transcript-checked`.
- **Disagreements** are to-do files, not edits to expectations.

## 3. Equipment checklist

| Item | Needed for | Status / note |
|---|---|---|
| IC-7300, USB cable, its menu photographed | A1, B1 to B9, C1 to C9 | on the bench 2026-10-31 |
| Toughpad (`ai@bruce-toughpad.lan`, Debian 13, Hamlib 4.6.2), `~/ic7300` with binaries | everything on the IC-7300; R14 (Hamlib 4.6) | `dialout` not set |
| Dev box (Hamlib 4.0.6; bladeRF, LimeSDR, NanoVNA-H, VNWA; **/dev/ttyUSB0 = SainSmart, never probe**) | B10, B11, A3, relay R1 | |
| Two 100 W dummy loads, one with the 50 dB sampler | every C check | ratings to be read (see 1.3) |
| Siglent SDS1202X-E scope, 192.168.10.51 | A3, C6, C8, C9, E3 | reachable from any machine |
| NanoVNA-H / VNWA 3E | A3 (sampler loss), C9 optional | VNWA max RX input unknown: scope first |
| Power meter or directional coupler | C6 cross-check, C7 | optional; the scope on the sampler replaces it |
| ATM6301 supply, 12 V load for it | E1 | |
| USB audio: the IC-7300's codec; a second interface (C-Media style) | B11, C8, R2, R6 | pi400-1's adapter did not enumerate 2026-10-06 |
| Flex 6300 (powered off 2026-10-30), LAN | B12, C10 | receive-only until Bruce says |
| KX3 with the correct cable | B13 | cable missing |
| K1EL WinKeyer hardware, or a USB-serial adapter with its key line looped to an input line | R1 | two readings, see R1 |
| Pi (pi400-1 / pi500-1) with a USB audio adapter | R2, U3 | |
| Windows laptop with real Chrome and an interactive logon; mac1 | R3, U1, U2 | |
| Shelly Plus 1, a Tasmota relay, Waveshare Modbus RTU relay + USB-RS485 | R10 to R12 | none confirmed in hand |
| Rotator controller (GS-232) with a rotator, Hamlib 4.6 machine | R14 to R16 | |
| Terminator, spare adapters, hand mic or key | C5 | |

## 4. Bench day order and time

Safety checks first, then receive-only, then low-power transmit into the dummy load, then everything else. Times include writing results down.

| Order | Phase | Checks | Minutes |
|---|---|---|---:|
| 1 | A. Safety and set-up, no RF | A1 to A6 | 40 |
| 2 | B. Receive only, gate closed | B1 to B9, B14 | 90 |
| 3 | B. Other receive-only hardware | B10 to B13 | 60 |
| 4 | C. Low-power transmit into the dummy load, Bruce present | C1 to C9 | 90 |
| 5 | E. bench_mcp instruments | E1 to E4 | 45 |
| 6 | D. Volunteer-only (not run on the day; listed so nobody claims them) | D1 to D3 | 0 |
| 7 | Relay side, hamlib dummy first then real hardware | R1 to R18, U1 to U5 | 240 |

Total about 5 h for radioctl and bench_mcp plus about 4 h for the relay side. Abort the day, not just the check, if A5 (the gate) or C2 (the watchdog) fails.

## 5. Index

Rig: **IC** = IC-7300 (Bruce's), **F63** = FlexRadio 6300, **KX3**, **any** = any attached rig, **vol** = a volunteer's rig only (Bruce does not own it).
"Present" = Bruce must be physically present.

| Id | Check | Rig | Phase | Min | Present | Source |
|---|---|---|---|---:|---|---|
| A1 | Menu settings recorded and set safe | IC | A | 10 | yes | IC plan Q1 to 5 |
| A2 | Port access and identity (dialout, by-id) | any | A | 5 | | IC plan item 0 |
| A3 | Dummy load, sampler and scope path measured with no radio | none | A | 15 | yes | IC plan, sampler note |
| A4 | Emergency stop within reach | any | A | 2 | yes | rule 4 |
| A5 | Gate closed: `T 1` refused, nothing on the wire | any | A | 5 | | IC plan step 6 |
| A6 | `radioctld --check` and `radioctl-selftest --list` | any | A | 3 | | VOLUNTEER_TESTING |
| B1 | radioctl-selftest, read-only, no upload | any | B | 5 | | VOLUNTEER_TESTING |
| B2 | Identify and read everything (`bench_console`) | IC | B | 10 | | IC plan step 2 |
| B3 | Front panel changes seen as events (Transceive vs Poll) | IC | B | 10 | | IC plan step 2 |
| B4 | Sets without transmitting, read back against the display | IC | B | 15 | | IC plan step 3 |
| B5 | Baud probe (`baud = "auto"`) | IC | B | 5 | | PR 1059 |
| B6 | Data modes USB-D / LSB-D | IC | B | 10 | | IC plan step 4 |
| B7 | Raw meters, receive only (`meter vd`) | IC | B | 5 | | IC plan step 4 |
| B8 | CI-V scope waveform | IC | B | 10 | | IC plan step 5 |
| B9 | Hamlib oracle on the real port | IC | B | 10 | | IC plan step 7 |
| B10 | bladeRF + XB-200 receive (bench_mcp) | none | B | 15 | | SDR notes |
| B11 | USB audio receive path (record, level) | IC | B | 15 | | IC plan step 9 |
| B12 | FlexRadio 6300 receive, VITA-49, `StopCw` not touched | F63 | B | 20 | | safety review |
| B13 | KX3 receive-only selftest | KX3 | B | 10 | | cable |
| B14 | radioctld for Hamlib clients (`rigctl -m 2`) | IC | B | 5 | | IC plan step 6 |
| C1 | PTT key and unkey, gate open for a bounded lease | IC | C | 10 | yes | IC plan step 8 |
| C2 | Maximum-transmit watchdog forces an unkey | IC | C | 5 | yes | IC plan step 8 |
| C3 | `kill -9` radioctld while keyed, restart unkeys first | IC | C | 10 | yes | PR 1049 |
| C4 | Unplug USB while keyed | IC | C | 10 | yes | IC plan step 8 |
| C5 | CW send and stop (`b TEXT`, `\stop_morse`) | IC | C | 10 | yes | IC plan step 8 |
| C6 | Split with `active_tx_frequency` while keyed; power on the scope | IC | C | 10 | yes | IC plan step 8 |
| C7 | Raw meters keyed, versus the front panel; power cap versus real RFPOWER | IC | C | 20 | yes | IC plan, relay 4c8e1f27 |
| C8 | Transmit audio tone through the USB codec into the load, ALC | IC | C | 15 | yes | relay a789ecff |
| C9 | Modulator splatter and harmonics at low drive (scope FFT) | IC | C | 10 | yes | IC plan step 9 |
| C10 | Flex `StopCw` (`cwx clear`, `cwx erase`, `xmit 0`) | F63 | C | 15 | yes | safety review 7c2e91a4 |
| C11 | Thermal sensor and duty cap on a rig that has TEMP_METER | IC | C | 15 | yes | relay 4c8e1f27 |
| D1 | Kenwood TS-590S/SG `KY` padding and stop | vol | D | - | | safety review 7c2e91a4 |
| D2 | Other Kenwood KY models (TS-480, TS-890S, TS-990S) | vol | D | - | | PR 1155 |
| D3 | Every other description, by volunteers with `radioctl-selftest` | vol | D | - | | VOLUNTEER_TESTING |
| E1 | ATM6301 supply: dry run, confirm, timed off, safe state | none | E | 15 | yes | bench_mcp 5be90a16 |
| E2 | Sound tools: dry run, safe state stops only its own playback | none | E | 10 | | bench_mcp 5be90a16 |
| E3 | `bench_scope_self_cal` dry run on the real scope | none | E | 5 | | bench_mcp 5be90a16 |
| E4 | `FULL_WITHOUT_SAFE_STATE` exemption confirmed; `verified:` lines | none | E | 15 | Bruce decides | bench_mcp 5be90a16 |
| R1 to R18 | Relay checks | | R | 240 | many | `RELAY_BENCH_CHECKLIST.md` |
| U1 to U5 | Self-update on other machines | | R | | | `RELAY_BENCH_CHECKLIST.md` |

Counting: 11 Bruce-rig checks in phase C and 14 in B, 6 in A, 4 in E, 3 volunteer-only (D), 23 relay and update checks: 61 in all.

## 6. Checks

Each check: **Needs** (equipment), **Steps** (in order), **Run** (the command), **Expect**, **Abort**, **Record**, **Unlocks**.
Directory for commands on the Toughpad: `~/ic7300` (the examples find `rigs/` there). `P` stands for the radio's `/dev/serial/by-id/...` path.

### Phase A. Safety and set-up (no RF)

#### A1. Menu settings recorded and set safe
- **Needs:** IC-7300, Bruce, a phone to photograph the menus. **Present:** yes.
- **Steps:** with the USB cable unplugged: photograph the CI-V block (CI-V address, CI-V baud, **CI-V USB baud**, CI-V transceive, **USB Echo Back**, **USB port**: Unlink/Link to
  REMOTE), the firmware (Version menu). Set **USB SEND = OFF, USB Keying (CW) = OFF, USB Keying (RTTY) = OFF, Echo Back = OFF**. Write every value into the day log.
- **Run:** none.
- **Expect:** values noted; the three keying items off so DTR/RTS cannot key the radio whatever the host does.
- **Abort:** if a keying item cannot be set to OFF, do not connect the USB cable.
- **Record:** day log header and the report header (`firmware:`); answers IC plan questions 1 to 5.
- **Unlocks:** the `firmware` and `civ.address`, `echo`, CI-V baud facts of `icom-ic-7300.toml` (need B1/B2 to run to become hardware-verified).

#### A2. Port access and identity
- **Needs:** Toughpad with the radio plugged in. **Steps:** `id -nG` (must list `dialout`); `lsusb` gains `10c4:ea60`; `dmesg | tail`; `ls -l /dev/serial/by-id`; `udevadm info` only.
  Never open `/dev/ttyUSB0` there (the FTDI cable). **Run:** the above. **Expect:** one new CP210x port, mode `root:dialout 660`.
  **Abort:** a second CP210x (two radios) or the port is not the CP210x: stop and identify. **Record:** the by-id path into the day log. **Unlocks:** nothing by itself.

#### A3. Dummy load, sampler and scope path measured with no radio
- **Needs:** dummy load with 50 dB sampler, scope (192.168.10.51), NanoVNA-H or VNWA, cables. **Present:** yes.
- **Steps:** read the sampler's and the scope input's ratings. With **no radio**, drive the sampler's input from the NanoVNA-H generator (or the VNWA generator at its lowest
  level) and measure the loss to the sampler port on the scope (`bench_measure`, `bench_screenshot`) at 7, 14 and 28 MHz. Use the scope first; connect the VNA to the sampler output
  only if the scope shows the sampler path is far below the VNWA's maximum RX input (that figure is unverified).
- **Expect:** about 50 dB, flat within a dB or two. **Abort:** any rating that 5 W at the load would exceed.
- **Record:** the measured loss per band into the day log; it is the correction used in C6, C7, C9. **Unlocks:** the calibration of every power reading below.

#### A4. Emergency stop within reach
- **Steps:** decide and say aloud: the radio's power switch is the stop. A hand is on it for every C check; a second person or Bruce stays within one arm's length. The USB cable is not the stop
  (C4 shows why). **Record:** a line in the day log. **Unlocks:** permission to start phase C.

#### A5. Gate closed: a transmit request is refused and nothing is on the wire
- **Needs:** radio connected, config with `transmit = false`. **Run:** `bin/radioctld --config ic7300-bench.toml &` then with `bench_console --wire` or a pty tap: `rigctl -m 2 -r 127.0.0.1:4532 T 1`.
- **Expect:** `RPRT -9`, no PTT frame in the wire capture. **Abort:** the radio keys: switch it off; stop the day. **Record:** wire capture. **Unlocks:** the gate fact; precondition for every C check.

#### A6. Configuration check and discovery list
- **Run:** `bin/radioctld --config ic7300-bench.toml --check`; `radioctl-selftest --list`. **Expect:** `ok`; the ports, their USB ids and the descriptions. `--list` opens no port.
  **Record:** the output to the day log. **Unlocks:** nothing; the runner's detection step reuses `--list`.

### Phase B. Receive only (gate closed)

#### B1. radioctl-selftest, read-only
- **Needs:** radio, A1/A2 done, no other program on the port. **Run:** `radioctl-selftest --description icom-ic-7300 --port usb:10c4:ea60 --baud 115200 --no-upload --out-dir day/`
  (use the radio's CI-V USB baud; `--baud auto` if unknown). It turns the VFO-knob check on at a terminal: turn the knob when asked.
- **Expect:** every claimed check `PASS`; the report file in `day/`. It sends only the read commands in `is_read_only`. **Abort:** a timeout on every command: baud, address, Echo Back (A1).
- **Record:** the report file. Uploading to hams.com is Bruce's choice after reading the report (`--upload` in the runner; the plain selftest uploads by default).
- **Unlocks:** `identify`, frequency/mode reads, meter/level reads in `icom-ic-7300.toml` as `hardware-verified` with `tested = "radioctl-selftest read-only"`.

#### B2. Identify and read everything
- **Run:** `bin/bench_console --log session-$(date +%H%M).txt --wire wire-$(date +%H%M).txt icom-ic-7300 usb:10c4:ea60 19200`; type `id` (expect `ModelId(148)`), `freq a`, `mode`, `filter`,
  `split`, `level af-gain`, `signal`, `power`. `help` lists the rest. **Expect:** all answer; `swr` (calibrated), `firmware`, `txvfo` refuse (not implemented on CI-V).
  **Abort:** none (receive only). **Record:** the log and wire files. **Unlocks:** the read facts, the `model_id` fact (assumed 94h), `echo = false`.

#### B3. Front panel changes arrive as events
- **Steps:** in `bench_console`, turn the main knob, change mode, press SPLIT, key nothing. **Expect:** frequency and mode as `event source=Transceive` within about 50 ms (if CI-V transceive is on),
  PTT/signal/split by `source=Poll` in 1 to 4 s. **Abort:** none. **Record:** the `event` lines. **Unlocks:** the `transceive` capability fact and the polling intervals.

#### B4. Sets without transmitting
- **Run (console):** `setfreq a ...`, `setmode usb`, `setfilter 2400`, `setsplit 1`, `setrit 1`, `setritoffset 100`, `setlevel af-gain 128`; compare each with the display and write down every `!` line.
- **Expect:** each reads back; BCD level scaling matches the display. **Abort:** if a set changes something unexpected (a band jump), stop. **Record:** the log. **Unlocks:** the set facts of the same names.

#### B5. Baud probe
- **Steps:** receive only, gate closed. In the config set `baud = "auto"` (or `baud_probe = [19200, 115200]`); run `bin/radioctld --config ic7300-bench.toml`; read the line `baud probe chose RATE`.
  **Expect:** RATE equals the radio's CI-V USB baud menu value; restore the fixed `baud` afterwards. **Abort:** `baud probe: no answer` (wrong address or Echo Back ON).
  **Record:** both values. **Unlocks:** the `baud_probe` fact (PR 1059).

#### B6. Data modes
- **Run (console):** `setmode data` then `mode`; the front panel must show USB-D. `setmode usb`: DATA goes off. `setmode data-r` for LSB-D. Through radioctld: `rigctl -m 2 -r 127.0.0.1:4532 M PKTUSB 0` then `m`.
  **Expect:** as above. Questions for the radio: does `06 01` clear DATA, does `26 00` work without the filter byte, does the filter number match FIL1/2/3. **Record:** answers. **Unlocks:** the data-mode facts.

#### B7. Raw meters, receive only
- **Run (console):** `meter vd`, `meter swr`, `meter alc`, `meter comp`, `meter id`. **Expect:** `vd` nonzero (note against the radio's Vd meter, menu METER); the others 0 unkeyed.
  **Record:** raw Vd against the display: the first calibration pair. **Unlocks:** read-only meter facts (Vd).

#### B8. CI-V scope waveform
- **Run (console):** `scope on`, `setspan 0 50000`, `setaxis 0 fixed`, `setedges 0 1 14000000 14350000`, `frames 20 sweeps.csv`; try CI-V USB baud 115200.
  **Expect:** about 475 samples per sweep; compare span and edges with the radio's display; note the frame rate (virtual: 9.8 per second). **Record:** `sweeps.csv`, rate. **Unlocks:** the scope facts.

#### B9. Hamlib oracle on the real port
- **Run:** stop radioctld; `rigctl -m 3073 -r P -s 19200 f m v s l AF` then the same through `rigctl -m 2` on radioctld. **Expect:** agreement on f, m, j, l AF, s; the dry run's known differences are `s` after split,
  `x`, `l STRENGTH` against `RAWSTR`, first `m` passband. **Record:** the differences as to-dos. **Unlocks:** nothing; evidence for the pinned open questions.

#### B10. bladeRF + XB-200 receive
- **Needs:** dev box (bladeRF FPGA is RAM-only; reload it). **Run:** bench_mcp `bench_sdr_capture` and `bench_sdr_survey` on a quiet band, with antenna port terminated.
  **Expect:** noise floor sensible, no clipping. **Abort:** none (receive only). **Record:** capture file. **Unlocks:** the bench receive path used by the noise study; no radioctl fact.

#### B11. USB audio receive path
- **Needs:** the IC-7300 USB codec (separate USB device; check `lsusb` and that plugging it does not renumber the serial port). **Run:** `bench_audio_discover`, `bench_audio_record` with `dry_run` first,
  then a few seconds of receive audio at low volume (07:00 to 22:00), `bench_audio_meter`. **Expect:** a level that follows the radio's AF/line-out setting; the system default device is never used.
  **Record:** level at two settings. **Unlocks:** the relay's per-rig audio facts (R6).

#### B12. FlexRadio 6300 receive
- **Needs:** the 6300 powered on, LAN, radio set to receive. **Run:** `radioctl-selftest --list` (it only listens for the discovery broadcast), then `radioctl-selftest --description flex-6300 ... --no-upload` once
  network testing is supported (today "Testing network radios is not supported yet": use `bench_console` with the TCP transport). **Expect:** identify and slice reads. **Abort:** any transmit or `cwx` command: **receive-only until Bruce says**.
  **Record:** log. **Unlocks:** Flex read facts (VITA-49 receive tests also unlock the shared-receiver plan).

#### B13. KX3 receive-only
- **Needs:** KX3 with the correct cable. **Run:** `radioctl-selftest --description elecraft-kx3 --port usb:... --baud 38400 --no-upload`; turn on AI2 (auto-information) for the VFO knob check. **Expect:** all claimed checks pass.
  **Record:** report. **Unlocks:** KX3 read facts; the first Elecraft family hardware verification.

#### B14. radioctld for Hamlib clients
- **Run:** `bin/radioctld --config ic7300-bench.toml &` then `rigctl -m 2 -r 127.0.0.1:4532 f m s l AF`; `\get_info`, `\get_active_tx_freq`, `\get_vfo_info` through `python3 dryrun/rc.py`. **Stop radioctld afterwards.**
  **Expect:** same answers as B2. **Record:** transcript. **Unlocks:** the Hamlib-facing facts (`dump_state`, rigctld replies).

### Phase C. Low-power transmit into the dummy load (Bruce present, TX-PROC)

Preconditions the runner enforces and a person must also check: A1, A3, A4, A5 passed; the dummy load seen on the antenna port; power at minimum; `max_transmit_s = 5`; B1 and B2 passed this session.

#### C1. PTT key and unkey, gate open for a bounded lease
- **Run:** `bin/radioctld --config ic7300-bench-tx.toml &`; `python3 dryrun/hold.py 8` (keys with `T 1`, polls `t` each second); then `(echo "T 1"; sleep 3; echo "T 0"; sleep 1) | nc 127.0.0.1 4532`.
- **Expect:** radio keys and unkeys; the gate is closed again after each unkey. **Abort:** it stays keyed: power switch. **Record:** wire capture, scope screenshot of the carrier. **Unlocks:** `ptt` (PTT via CI-V `1C 00`, awaiting-ACK behaviour).

#### C2. The maximum-transmit watchdog
- **Run:** same config (`max_transmit_s = 5`); `hold.py 8`. **Expect:** `t` reads 0 at about 5 s; the radio is unkeyed by the daemon. **Abort/record:** as C1. **Unlocks:** watchdog fact.

#### C3. `kill -9` radioctld while keyed
- **Run:** `T 1`, `kill -9` the daemon (hand on the power switch: the radio stays keyed until its own limit), restart. **Expect:** the restart sends an unkey first and refuses to serve a radio it cannot unkey (PR 1049).
  **Abort:** the radio is still keyed 2 s after restart. **Record:** log. **Unlocks:** startup unkey fact.

#### C4. Unplug USB while keyed
- **Run:** `T 1`, pull the cable. **Expect:** `RPRT -6` for `t`, `f`, `T 1`, `T 0` during the outage; the radio's own behaviour is recorded (it may stay keyed; hand on the switch); after replug (the node may change: `usb = {...}`) radioctld
  reconnects and unkeys. **Record:** what the radio did while unplugged. **Unlocks:** link-loss fact; informs the relay's R4.

#### C5. CW send and stop
- **Run:** `M CW`, `b TEST`, `\stop_morse` (CI-V `17` send, `17 FF` stop). **Expect:** a short keyed carrier; stop ends it. **Abort:** it keeps sending after stop: switch off. **Record:** scope capture. **Unlocks:** `send-cw` and `StopCw` for the IC-7300.

#### C6. Split with the transmit frequency while keyed
- **Run:** `F`, `I`, `S 1 VFOB`, `T 1`, `\get_active_tx_freq`. **Expect:** `f` returns VFO A, `\get_active_tx_freq` VFO B; the scope on the sampler shows the carrier at VFO B (apply the A3 loss).
  **Unlocks:** the `active_tx_frequency` fact.

#### C7. Meters keyed; power cap versus real RFPOWER
- **Run:** with `bench_console` on the port (it cannot key), key with the hand mic or `rigctl -m 3073 T 1` in another terminal; `meter swr`, `meter alc`, `meter id`, `meter vd`, `power` at 2 or 3 power settings; compare with the front panel
  multi-function meter and with the scope on the sampler (A3 correction). Then set the relay's `set_rig_power_cap` and read `RFPOWER` at the cap (relay side R9).
- **Expect:** raw values rise with power and agree with the panel. **Abort:** SWR above 1.5 into the load: stop (connector). **Record:** raw-versus-display pairs: the start of radioctl's own `meter_cal`.
  **Unlocks:** the `read-meters`, `read-swr` facts (to-do `radioctl-civ-swr-alc-meter-reads`); calibration points stay `unverified` until at least three powers are recorded.

#### C8. Transmit audio tone through the USB codec
- **Needs:** B11 passed, scope on the sampler. **Run:** bench_mcp `bench_audio_play` (tone) with `dry_run` first, mode USB, data/mic settings on; start low. **Expect:** RF at the tone offset; ALC deflection; no RF with silence.
  **Abort:** ALC reads high at the lowest level: stop. **Record:** audio level, RF level, ALC. **Unlocks:** relay audio-to-TX facts (R6).

#### C9. Modulator splatter and harmonics
- **Run:** scope FFT (`bench_scope_spectrum`) on the sampler at 5 W, SSB tone and CW. **Expect:** harmonics at least 40 dB below the carrier on the sampler (document the figure; the radio's own spec is the limit). **Record:** screenshots. **Unlocks:** nothing in a description;
  evidence for the drive policy (relay R17).

#### C10. Flex `StopCw`
- **Needs:** Bruce's go-ahead to transmit on the 6300 (receive-only otherwise). **Run:** dummy load, power minimum, `cwx send "TEST"` then `cwx clear` (the current `StopCw`); see whether an element already keyed ends, else try `cwx erase`, `xmit 0`.
  **Expect:** the carrier ends immediately on one of them. **Record:** which. **Unlocks:** Flex `StopCw` (fix the description if `cwx clear` alone is not enough).

#### C11. Thermal sensor and duty cap
- **Needs:** a rig that reports TEMP_METER. **Run:** relay `tx_limits` with a low temperature limit; key briefly; the rig unkeys and a key-on is refused. Per-mode duty cap with a short window.
  **Abort:** any real heating. **Record:** readings. **Unlocks:** the relay's thermal and duty facts (R8).

### Phase D. Volunteer-only (Bruce owns no such rig)

- **D1. Kenwood TS-590S/SG `KY` padding and stop.** A volunteer with the radio sends `KY` text of lengths 1 to 24 and checks the 24-character padding and the stop; transmit procedure of VOLUNTEER_TESTING.md.
- **D2. TS-480, TS-890S, TS-990S `KY`** (accepted by PR 1155); the same.
- **D3. Every other description.** `radioctl-selftest` (VOLUNTEER_TESTING.md), then the maintainer promotes facts. Listed here so no one marks them by guess.

### Phase E. bench_mcp instruments (checks the legacy hardening only tested against fakes)

#### E1. ATM6301 supply
- **Needs:** supply, dummy load (a resistor, 12 V). **Run:** `bench_psu_output` with `dry_run`; refusal without `confirm_load_connected`; timed off at `for_s` 1 and 5 (server thread, not the supply); `bench_psu_safe_state` reads `OUTP?` (`ON`/`OFF`; the manual says 0/1, both accepted);
  an explicit off cancels a pending timer. Look for a supply-side output timer or OVP/OCP preset. **Known limit:** if the server dies while on, nothing switches it off. **Abort:** any output when the dry run says none. **Unlocks:** the supply pages' `verified:` line.

#### E2. Sound tools
- **Run (07:00 to 22:00, low volume):** `bench_audio_play`, `_speak`, `_alert`, `_loopback`, `_set_volume` with `dry_run: true`; `bench_audio_safe_state` stops a running `pw-play -P node.dont-fallback=true ...` and touches no other program; a call with no `output` is refused on the dev box.

#### E3. Scope self-calibration dry run
- **Run:** `bench_scope_self_cal` with `dry_run` on the real unit. **Expect:** sends nothing.

#### E4. Exemption and pages
- **Decide:** confirm `bench_scope_self_cal` in `FULL_WITHOUT_SAFE_STATE` or name a companion; then update the `verified:` lines of the pages for what ran, with the date. **Unlocks:** the pages' level from `code only`.

### Phase R. Relay side

R1 to R18 and U1 to U5 are in `docs/runbooks/RELAY_BENCH_CHECKLIST.md` (hams_com), with the same fields.

## 7. After the day

1. Run `python3 daemons/radioctl/tools/bench_verify.py report` (or collect the files by hand) and copy the day's captures out of the Toughpad (`rsync`).
2. For each `PASS` with TX-PROC where needed, flip the facts it unlocks and add the `[[report]]` (the runner's `verified.toml.proposed`). A failed check becomes a to-do file, not an edited expectation.
3. Kill leftovers (rule 8), delete the Toughpad build artefacts you made, retire or update the night_shift_todo files named in the Source column.

## 8. The guided runner

`python3 daemons/radioctl/tools/bench_verify.py verify <rig-id>` detects the attached rigs, prints the checks above for that rig, runs the receive, CAT and meter checks, and writes
`BENCH-REPORT-<rig>-<date>.txt` and `verified.toml.proposed`. Transmit checks are dry runs (they print what they would do) unless `--transmit` is given **and** the operator types the phrase
`DUMMY LOAD ATTACHED` when asked; they are refused while any safety precondition fails. See the tool's `--help` and its tests.
