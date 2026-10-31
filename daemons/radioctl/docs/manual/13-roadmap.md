# 13. Roadmap and open items

Everything in the first table is **planned**: described in a proposal or a decision, with no code on
`main`. The proposals are in the private `hams_com` repository (`docs/proposals/`):
`RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md` (RC), `RELAY_SDR_RADIOCTL_ROADMAP.md` (the roadmap),
`RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md` (switches) and `HAMS_USB_DEVICE_PROTOCOL.md` (USB).
Do not build on a planned item as if it existed.

## Planned

| Item | Source | Status in code |
|---|---|---|
| **Safety:** per-rig dead-man timer; forced unkey for a crashed or hung process; carrier drop after link loss; maximum transmit duration; ownership modes (shared receive, exclusive transceive, transmit-locked); a mismatch event between intended and actual transmit state | RC section 8; roadmap 2a items 2 to 4 | the actor unkeys after an unanswered key and when its last handle is dropped; the rest none |
| **Transmit frequency for the remaining radios:** a transmit-VFO report for Yaesu legacy, a model for band-wise split (FTDX10, FTDX101D), the follower test on a real radio | RC section 14 (Hamlib issue 138) | implemented for Kenwood, Elecraft, Yaesu newcat and CI-V on virtual radios (chapter [4](04-api.md)); Yaesu legacy has a `TODO` |
| **Scope through the API:** a scope `Op`, scope events, scope settings, non-Icom scopes | RC, roadmap track E | CI-V scope decoder exists, unreachable from `RigHandle` |
| **Capability traits** per capability, typed methods for every operation, memories, functions (NB, NR, preamp, attenuator), tuner and amplifier, power on and off | RC section 4 | one `Op` enum and `Capability` set |
| **Raw-command escape hatch** behind its own named capability | RC section 4 | none; `radioctld` refuses `send_cmd` |
| **Rig description additions:** command table, meter scaling and calibration to dB and watts, firmware differences, a per-model override trait, capability discovery by probing (ask the radio what it is) | RC section 5 | the format has none of these |
| **Rig lifecycle** `Off`, `PoweringOn`, `Booting`, `Ready`, `Degraded`, `Unreachable`, with cached state served with its age while off; reconnect that waits for hot-plug, a ping or the boot time | switches section 5 | only serial reconnect below the actor |
| **Kenwood TS-890S** dialect (no `IF`, `OM` for mode, bandscope); Elecraft K3 and K4 descriptions; the FT-818 (no manual found) | RC section 9 | none |
| **Network and SDR radios, beyond what exists:** FlexRadio meters, spectrum and audio (VITA-49 streams), discovery on the wire, the 8000 series; HPSDR/ANAN as a radioctl rig (Macdoel's ANAN-100, end of November 2026); Kenwood LAN, Icom LAN; shared receivers as virtual rigs (`radiod`) | RC sections 3.5, 9; roadmap tracks C, D | the Flex control-plane engine exists (chapter [7](07-protocol-engines.md)); a separate `daemons/hpsdr` crate holds an unverified Protocol 1 client and emulator, with no radioctl engine, description or integration |
| **Native non-serial USB** radios (`nusb`) and the **Hams USB device protocol** (`GET_INFO`, sub-ID registry; vendor 0x1209, product 0x7373 chosen, nothing registered) | RC section 3.5; USB proposal | no radioctl code; the FX2LP firmware bring-up is in `hardware/fx2_station_io/` (test PID, host loader), and its README says the radioctl host driver is future |
| **Station module:** external switches, power control, durable radio records, a power button, profiles, interlocks, virtual switches with fault injection (decided: a radioctl module, no sibling crate) | switches sections 3 to 9 | none in radioctl (the FX2LP firmware above is the first hardware piece) |
| **C ABI** for non-Rust callers; Python bindings; an isolated mode (the API over IPC, one rig per process) | RC section 7 | `owned-runtime` feature only; no ABI |
| **Rotators, amplifiers, tuners, keyers** (WinKeyer) on the same machinery | RC section 7; whether they are in release one is still open | none |
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

1. **Link state is invisible above the serial layer.** During a serial reconnect `RigHandle::link_up()` stays
   true and `radioctld` drops the `SerialLink`, so a pulled cable looks like timeouts; `\get_powerstat`
   keeps answering 1. A dead TCP link is permanent for that actor.
2. **Polling reads VFO A, VFO B, split, mode, PTT and the two meters.** A front-panel change of RIT, XIT or
   a level is not noticed unless the radio reports it.
3. **CI-V transceive cannot be switched on by the engine** (`SetTransceive` is `Unsupported`; it is a menu
   item). The Icom descriptions nevertheless list the capability.
4. **`radioctld` `\dump_caps` emits empty `agc_levels=`, `ctcss_list=` and `dcs_list=` lines**; Hamlib's
   `rigctl -vvv` logs "invalid dumpcaps line" for them. Hamlib's own `rigctld` does not. Harmless to the
   oracle tests; not understood.
5. **Oracle disagreements to settle on real radios:** TS-590 VFO B read (`FA;` or `FB;`) and AF gain,
   IC-7610 split flow, IC-9700 VFO B, X108G final VFO A.
6. **The TS-2000 splitter personality still refuses `IF;`**, though the Kenwood engine decodes it.
7. **Stale comments in the code:** `radioctl-core/src/transport.rs` says no real transport exists;
   `radioctl-core/src/description.rs` says the TOML format arrives in a later PR; `docs/DESIGN.md` item 16
   still says the TS-2000 personality refuses `IF` for an unverified layout. Fix them with the files'
   owners; the manual follows the code.
8. **`radioctld` serial transport is fixed** at 8N1 with one open attempt and default idle lines; there is
   no configuration for data bits, parity, flow control or reopen attempts.
9. **Nothing has run on hardware.** All 217 facts in the 21 descriptions are `unverified`; no client other
   than Hamlib's `rigctl` has been tried against `radioctld`; the splitter has not served a real program;
   no Windows build exists.

10. **The Flex engine reads no meters**, and the legacy Yaesu PTT-bit level is a single Hamlib fact; a
    wrong guess inverts the PTT read. Both need a bench.

Update this chapter whenever one of these changes.
