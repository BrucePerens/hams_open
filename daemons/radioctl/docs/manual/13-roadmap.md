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
| **Rig description additions:** command table, meter scaling and calibration to dB and watts, firmware differences, a per-model override trait, capability discovery by probing (ask the radio what it is) | RC section 5 | the format has none of these |
| **Rig lifecycle** `Off`, `PoweringOn`, `Booting`, `Ready`, `Degraded`, `Unreachable`, with cached state served with its age while off; reconnect that waits for hot-plug, a ping or the boot time | switches section 5 | only serial reconnect below the actor |
| **Kenwood TS-890S** bandscope (the dialect engine and the LAN session `kenwood_ts890_lan` exist and `radioctld` serves the session as `framing = "ts890-lan"`, fake radio only; chapter [5](05-transports.md#the-ts-890s-over-the-lan)); Elecraft K3 and K4 descriptions | RC section 9 | none |
| **Network and SDR radios, beyond what exists:** FlexRadio meters, spectrum and audio (VITA-49 streams), discovery on the wire, the 8000 series; the OpenHPSDR I/Q data plane (the attenuator is a core level now) in radioctl (Macdoel's ANAN-100, end of November 2026); Kenwood LAN, Icom LAN; shared receivers as virtual rigs (`radiod`) | RC sections 3.5, 9; roadmap tracks C, D | the Flex and OpenHPSDR control-plane engines exist, unverified (chapter [7](07-protocol-engines.md)); the `hpsdr` crate in `daemons/hpsdr` holds the Protocol 1 and 2 clients and emulators |
| **Native non-serial USB** radios (`nusb`) and the **Hams USB device protocol** (`GET_INFO`, sub-ID registry; vendor 0x1209, product 0x7373 chosen, nothing registered) | RC section 3.5; USB proposal | no radioctl code; the FX2LP firmware bring-up is in `hardware/fx2_station_io/` (test PID, host loader), and its README says the radioctl host driver is future |
| **Station module:** external switches, power control, durable radio records, a power button, profiles, interlocks, virtual switches with fault injection (decided: a radioctl module, no sibling crate) | switches sections 3 to 9 | the `radioctl-station` crate (chapter [14](14-station.md)): model, switchboard, interlocks, lifecycle, radio manager over real rig actors, Modbus, Shelly/Tasmota and Home Assistant drivers, amplifier channels; no hardware has run any of it |
| **C ABI** for non-Rust callers; Python bindings; an isolated mode (the API over IPC, one rig per process) | RC section 7 | a sketch exists: `radioctl-capi` (chapter [14](14-station.md#the-c-abi)): runtime and rig handles, frequency, mode, PTT behind the permit call; no rotators, no events, no Python |
| **Rotators, amplifiers, tuners, keyers** (WinKeyer) on the same machinery | RC section 7; whether they are in release one is still open | rotators (`rotctld` and GS-232) and amplifier and tuner channels exist in `radioctl-station` (chapter [14](14-station.md)), and `radioctld` serves rotators in the `rotctld` protocol (chapter [9](09-radioctld.md#rotators), virtual rotator only); tuners as rig operations, and keyers (WinKeyer), do not |
| **Splitter integration in `radioctld`**, one actor shared by both | not in a proposal | the splitter is a library with examples |
| **Coverage-guided fuzzing** of every parser | RC section 8 | proptest only |
| **Relay integration** behind a per-radio switch with Hamlib fallback in a per-rig process; removing `HAMLIB_CALL_LOCK` | RC section 9 | not in this repository |

The first physical radio for radioctl is the **KX3** (roadmap decision, 2026-10-30), with a bladeRF/XB-200 or
LimeSDR as the measuring receiver for conducted tests; its bench run is what upgrades the first `verified`
fields. The first families, by decision: Icom, Kenwood, Elecraft, Yaesu, FlexRadio and Xiegu.

## Decisions still open (from the proposals)

Location and name (the default, `hams_open/daemons/radioctl`, is in use); whether rotators and amplifiers
are in the first release; exactly which radios Bruce and members can bring to a bench, per family; the
priority against the zero-privilege, SDR and band-scope tracks (the roadmap puts radioctl first).

## Known gaps and findings in the code

Found while writing this manual. Each is a statement about `main` on 2026-10-30, none is fixed by the
manual.

1. **Reconnect and session state.** Link loss and restore reach the rig actor on every link `radioctld` and `radioctl-capi` open (serial through `open_serial`, TCP and UDP through `open_link`; `RigOptions::link_watch`), which unkeys on a restored link. A bare `connect_tcp` stream (used by tests and by a caller that builds its own) is a dead link for good. `radioctld`'s `tcp` transport has been on `net::open_link` since 2026-10-31 (reconnects, the actor watches the link), and since 2026-11-01 the actor tells the engine when the link is back: after the fail-closed unkey and before the `LinkRestored` event it calls `ProtocolEngine::link_restored`, which drops half-received frames and returns the requests that set the session up again, each written once. The Kenwood (including TS-890S; its LAN login is the framing's and repeats on every reconnect), Elecraft and Flex engines re-send `AI2;` or the `sub` lines only if the controller had turned them on; the Icom PCR receivers (`receiver.rs`) also have an open step, `ProtocolEngine::open_requests`: `H101` and `G300` are written once when the actor starts with the link up and again after each reconnect, before any command, never keying anything (decision 24 in `DESIGN.md`); every other engine sets up nothing. Still open: a CI-V radio's transceive (a menu item) cannot be restored by the engine.
2. **Polling reads VFO A, VFO B, split, mode, PTT and the two meters.** A front-panel change of RIT, XIT or
   a level is not noticed unless the radio reports it.
3. **CI-V transceive cannot be switched on by the engine** (`SetTransceive` is `Unsupported`; it is a menu
   item). The Icom descriptions nevertheless list the capability.
4. Fixed 2026-10-30: `\dump_state` no longer prints empty `agc_levels=`, `ctcss_list=` and `dcs_list=`
   lines (Hamlib's `rigctld` prints each only when non-empty; the empty ones were "invalid dumpcaps
   line" to `rigctl -vvv`).
5. **Oracle disagreements to settle on real radios:** TS-590 VFO B read (`FA;` or `FB;`) and AF gain,
   IC-7610 split flow, IC-9700 VFO B, X108G final VFO A.
6. **The TS-2000 splitter personality still refuses `IF;`**, though the Kenwood engine decodes it.
7. Fixed 2026-10-30: the stale module notes in `transport.rs` and `description.rs` and the `IF` wording of
   `docs/DESIGN.md` item 16 (the personality still does not answer `IF`; the reason is a missing encoder,
   not an unverified layout).
8. **`radioctld` serial transport is fixed** at 8N1 with one open attempt and default idle lines; there is
   no configuration for data bits, parity, flow control or reopen attempts.
9. **Nothing has run on hardware.** All 233 facts in the 23 descriptions are `unverified`; no client other
   than Hamlib's `rigctl` has been tried against `radioctld`; the splitter has not served a real program;
   no Windows build exists.

10. **The Flex engine reads no meters**, and the legacy Yaesu PTT-bit level is a single Hamlib fact; a
    wrong guess inverts the PTT read. Both need a bench.

Update this chapter whenever one of these changes.
