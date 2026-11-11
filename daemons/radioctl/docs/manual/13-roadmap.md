# 13. Roadmap and open items

Everything in the first table is **planned**: described in a proposal or a decision, with no code on
`main`. The proposals are in the private `hams_com` repository (`docs/proposals/`):
`RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md` (RC), `RELAY_SDR_RADIOCTL_ROADMAP.md` (the roadmap),
`RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md` (switches) and `HAMS_USB_DEVICE_PROTOCOL.md` (USB).
Do not build on a planned item as if it existed.

## Planned

| Item | Source | Status in code |
|---|---|---|
| **Safety, remaining:** carrier drop after link loss (a documented, tested time); ownership modes (shared receive, exclusive transceive, transmit-locked); a mismatch event between intended and actual transmit state; an unkey for a killed process (the FX2 hardware time-out and switched power are the outer layers) | RC section 8; roadmap 2a items 2 to 4 | the dead-man watchdog, heartbeat, alarms and fail-closed unkeys are implemented (chapter [10](10-safety.md)); the items here are not |
| **Transmit frequency for the remaining radios:** a transmit-VFO report for Yaesu legacy, a model for band-wise split (FTDX10, FTDX101D), the follower test on a real radio | RC section 14 (Hamlib issue 138) | implemented for Kenwood, Elecraft, Yaesu newcat and CI-V on virtual radios (chapter [4](04-api.md)); Yaesu legacy has a `TODO` |
| **Scope beyond the Icom CI-V operations:** non-Icom scopes, hold and speed through `Op`, the IC-705 and IC-9700 descriptions listing the capability, the relay's scope source | RC, roadmap track E | core scope operations and a sweep stream exist for the CI-V engine (chapter [4](04-api.md#scope-and-spectrum)) |
| **Capability traits** per capability, typed methods for every operation, memories, functions (NB, NR, preamp, attenuator), tuner and amplifier, power on and off | RC section 4 | one `Op` enum and `Capability` set |
| **Raw-command escape hatch** behind its own named capability | RC section 4 | none; `radioctld` refuses `send_cmd` |
| **Rig description additions:** command table, meter scaling and calibration to dB (the power meter's calibration to watts exists as `[power_meter]`, chapter [15](15-power.md)), firmware differences, a per-model override trait, capability discovery by probing (ask the radio what it is) | RC section 5 | the format has none of the others; `radioctl-selftest` discovers radios on the computer by port text and read-only identify commands, which is not a description feature |
| **Rig lifecycle** `Off`, `PoweringOn`, `Booting`, `Ready`, `Degraded`, `Unreachable`, with cached state served with its age while off; reconnect that waits for hot-plug, a ping or the boot time | switches section 5 | the six states are a sans-IO state machine in `radioctl-station` (`lifecycle.rs`), driven by the radio manager (chapter [14](14-station.md)); below the rig actor there is serial, TCP/UDP and USB reconnect with backoff |
| **Kenwood TS-890S** bandscope (the dialect engine and the LAN session `kenwood_ts890_lan` exist and `radioctld` serves the session as `framing = "ts890-lan"`, fake radio only; chapter [5](05-transports.md#the-ts-890s-over-the-lan)); Elecraft K3 and K4 descriptions | RC section 9 | none |
| **Network and SDR radios, beyond what exists:** FlexRadio meters, spectrum and audio (VITA-49 streams), discovery on the wire, the 8000 series; the OpenHPSDR I/Q data plane (the attenuator is a core level now) in radioctl (Macdoel's ANAN-100, end of November 2026); Kenwood LAN, Icom LAN; shared receivers as virtual rigs (`radiod`) | RC sections 3.5, 9; roadmap tracks C, D | the Flex and OpenHPSDR control-plane engines exist, unverified (chapter [7](07-protocol-engines.md)); the `hpsdr` crate in `daemons/hpsdr` holds the Protocol 1 and 2 clients and emulators |
| **The Hams USB device protocol** (`GET_INFO`, sub-ID registry; vendor 0x1209, product 0x7373 chosen, nothing registered) | USB proposal | no radioctl code; the FX2LP firmware bring-up is in `hardware/fx2_station_io/` (test PID, host loader), and its README says the radioctl host driver is future. Existing USB radios and boards are not planned but done, as a transport behind the `UsbBackend` trait with a libusb backend (`rusb`, cargo feature `usb`, chapter [5](05-transports.md#usb-devices)); a different native backend such as `nusb` is not written |
| **Station module:** external switches, power control, durable radio records, a power button, profiles, interlocks, virtual switches with fault injection (decided: a radioctl module, no sibling crate) | switches sections 3 to 9 | the `radioctl-station` crate (chapter [14](14-station.md)): model, switchboard, interlocks, lifecycle, radio manager over real rig actors, Modbus, Shelly/Tasmota and Home Assistant drivers, amplifier channels; no hardware has run any of it |
| **C ABI** for non-Rust callers; Python bindings; an isolated mode (the API over IPC, one rig per process) | RC section 7 | a sketch exists: `radioctl-capi` (chapter [14](14-station.md#the-c-abi)): runtime and rig handles, frequency, mode, PTT behind the permit call; no rotators, no events, no Python |
| **Rotators, amplifiers, tuners, keyers** (WinKeyer) on the same machinery | RC section 7; whether they are in release one is still open | rotators (`rotctld` and GS-232) and amplifier and tuner channels exist in `radioctl-station` (chapter [14](14-station.md)), and `radioctld` serves rotators in the `rotctld` protocol (chapter [9](09-radioctld.md#rotators), virtual rotator only); tuners as rig operations, and keyers (WinKeyer), do not |
| **Splitter integration in `radioctld`**, one actor shared by both | not in a proposal | the splitter is a library with examples |
| **Coverage-guided fuzzing** of every parser | RC section 8 | seven cargo-fuzz targets in `fuzz/` (60-second runs only, clean), beside the proptests; no long runs or kept corpus |
| **Relay integration** behind a per-radio switch with Hamlib fallback in a per-rig process; removing `HAMLIB_CALL_LOCK` | RC section 9 | not in this repository |

The first physical radio for radioctl was the **KX3** (roadmap decision, 2026-10-30). Its bench run on
2026-11-05 upgraded the first facts to `hardware-verified` (`rigs/reports/verified.toml` records it;
chapter [6](06-rig-descriptions.md)). The first families, by decision: Icom, Kenwood, Elecraft, Yaesu,
FlexRadio and Xiegu. The open hardware checks of the other families are in `docs/BENCH_VERIFICATION.md`.

## Decisions still open (from the proposals)

Location and name (the default, `hams_open/daemons/radioctl`, is in use); whether rotators and amplifiers
are in the first release; exactly which radios Bruce and members can bring to a bench, per family; the
priority against the zero-privilege, SDR and band-scope tracks (the roadmap puts radioctl first).

## Known gaps and findings in the code

Found while writing this manual. Each was a statement about `main` on 2026-10-30; items 1, 9 and 11 were
brought up to date on 2026-11-08 and items 2, 3, 6, 8 and 10 were re-read against the code on 2026-11-10 (items 4 and 7 are
fixed and kept as a record); item 5 waits for a bench. The manual fixes none of them.

1. **Reconnect and session state.** Link loss and restore reach the rig actor on every link `radioctld` and `radioctl-capi` open (serial through `open_serial`, TCP and UDP through `open_link`; `RigOptions::link_watch`), which unkeys on a restored link. A bare `connect_tcp` stream (used by tests and by a caller that builds its own) is a dead link for good. `radioctld`'s `tcp` transport has been on `net::open_link` since 2026-10-31 (reconnects, the actor watches the link), and since 2026-11-01 the actor tells the engine when the link is back: after the fail-closed unkey and before the `LinkRestored` event it calls `ProtocolEngine::link_restored`, which drops half-received frames and returns the requests that set the session up again, each written once. The Kenwood (including TS-890S; its LAN login is the framing's and repeats on every reconnect), Elecraft and Flex engines re-send `AI2;` or the `sub` lines only if the controller had turned them on; the Icom PCR receivers (`receiver.rs`) also have an open step, `ProtocolEngine::open_requests`: `H101` and `G300` are written once when the actor starts with the link up and again after each reconnect, before any command, never keying anything (decision 24 in `DESIGN.md`); every other engine sets up nothing. Still open: a CI-V radio's transceive (a menu item) cannot be restored by the engine.
2. **Polling reads what the capabilities allow, and only these fields:** VFO A and VFO B frequency, split, mode,
   PTT (the fast group), the S-meter and the transmit power (the slow group), from `PollSchedule::for_capabilities`;
   the SWR, ALC and the other meters are read on request, not polled. A front-panel change of RIT, XIT or a level is
   not noticed unless the radio reports it (transceive) or a client reads it.
3. **CI-V transceive cannot be switched on by the engine** (`SetTransceive` is `Unsupported`; it is a menu
   item). The Icom descriptions nevertheless list the capability.
4. Fixed 2026-10-30: `\dump_state` no longer prints empty `agc_levels=`, `ctcss_list=` and `dcs_list=`
   lines (Hamlib's `rigctld` prints each only when non-empty; the empty ones were "invalid dumpcaps
   line" to `rigctl -vvv`).
5. **Oracle disagreements to settle on real radios:** TS-590 VFO B read (`FA;` or `FB;`; the Kenwood
   command reference defines `FR` as the receive VFO (simplex) and `FT` as the transmit VFO (split) but
   does not say which VFO's frequency `IF` carries while keyed in split, so a bench read is still needed) and AF gain,
   IC-7610 split flow, IC-9700 VFO B, X108G final VFO A.
6. Fixed 2026-11-01: the TS-2000 splitter personality answers `IF;` (`Request::ReadStatus`,
   `Outcome::Status`, `Ts2000Personality::status_answer`), built from the rig's frequency, mode, PTT, RIT,
   XIT, offset and split. Since 2026-11-09 (hams_open #1396) it reports the receive VFO's frequency and
   function digit rather than always VFO A. Not yet: pushing `IF` to an `AI2` client when those change.
7. Fixed 2026-10-30: the stale module notes in `transport.rs` and `description.rs` and the `IF` wording of
   `docs/DESIGN.md` item 16.
8. **`radioctld` serial transport is fixed** at 8N1 without flow control. The configuration chooses the
   path or USB identity, the baud rate (fixed, `"auto"` or a `baud_probe` list) and the idle lines
   (`lines = "unmanaged"`); there is no setting for data bits, parity or flow control, and the link
   reopens with backoff after an unplug.
9. **Almost nothing has run on hardware.** Only the Elecraft KX3 has (the bench run of 2026-11-05, including
   a keyed transmission into a dummy load); every other description is `unverified` or `manual-checked`
   (the counts are in [`RIG_SUPPORT.md`](../RIG_SUPPORT.md)). No client other than Hamlib's `rigctl` has
   been tried against `radioctld`; the splitter has not served a real program.

10. **The Flex engine reads no meters**, and the legacy Yaesu PTT-bit level is a single Hamlib fact; a
    wrong guess inverts the PTT read. Both need a bench.

11. **Perseus receiver board.** `radioctl-protocols::perseus` loads the user's own Microtelecom FX2
    firmware (`fx2.hex`, Intel HEX) and FPGA bitstream (`fpga.rbs`) into a board over the USB
    transport traits and controls frequency, attenuator, preselector, ADC dither and preamplifier,
    tested against a virtual board only. Nothing of Microtelecom's is stored or redistributed; the
    files are looked up in `RADIOCTL_PERSEUS_FIRMWARE_DIR` or `~/.config/radioctl/firmware/perseus`.
    It is not a `ProtocolEngine`; `radioctld` opens the board through its `usb` transport
    (chapter [5](05-transports.md#usb-devices); the `usb` cargo feature) with the
    `microtelecom-perseus-usb` description: the loader finds the firmware, the board is tuned and
    it is never keyed. Tested against the virtual board only. **Boundary:** I/Q streaming
    (bulk endpoint 0x82, sample-rate bitstream choice) belongs to the shared-receiver service
    (`radiod`), which this board feeds; radioctl stops at control.

Update this chapter whenever one of these changes.
