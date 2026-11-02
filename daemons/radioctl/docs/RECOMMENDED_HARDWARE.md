# Recommended hardware for radioctl-station

Started 2026-11-01. Nothing on this list is verified by us on hardware yet. Entries are provisional.

## 1. Why a list, and how to read it

- radioctl keeps its own record of every switch it drives (`Recorded`); a read-back upgrades it to `Confirmed`.
- So the question for a product is: can it be driven safely?
- Station actions that need a confirmed state (amplifier power-on, operate) refuse on a channel that can only be `Recorded`.

Criteria:

- State read-back: the device can report whether its output is on.
- Power-up state: defined, and settable by the owner.
- Failsafe: a watchdog, auto-off or pulse timer in the device.
- Local control: LAN or USB only, no cloud account.
- Protocol: documented publicly.
- Isolation and ratings: suitable for the load (stated, not assumed).
- Identity: stable USB identity (serial number) or fixed network address.
- Price and availability.
- Open firmware, where relevant.

Status of an entry:

- Recommended: meets the criteria on paper.
- Acceptable: usable, with a named gap; see the notes.
- Not recommended: fails a criterion that matters for safety.
- Every entry also says "verified by us on hardware": today always **no**.
- "unconfirmed" means a public page we read did not state it. Check the product before you rely on it.

## 2. Not recommended

### SainSmart 16-channel 9-36 V USB relay module (sku 101-70-208)

Status: **Not recommended.** Verified by us on hardware: no.

| Criterion | Finding | Kind |
|---|---|---|
| State read-back | Assumed not available. Bruce's decision (2026-11-01): assume it cannot report status. radioctl never sends function 01 to it and keeps its own record (assurance `Recorded`). | decision |
| State read-back, vendor | We have not seen a vendor status query. The vendor PDF (`16CH_USB_Relay_Instruction-EN.pdf`, inside the 101-70-208.zip on [SainSmart docs](https://docs.sainsmart.com/article/790hrr4nux-sain-smart-relay-module-sensors)) was not opened for this list. | unconfirmed |
| State read-back, third party | A [public gist](https://gist.github.com/omenlabs/3b795dc50e8ac985d6e8b4161131c613) sends a function-01 frame (`:FE0100000010F1`). It never shows or interprets a reply. Does not change the decision above. | third party |
| Watchdog or failsafe | None known. | assumption |
| Power-up state | Unknown and not settable. radioctl writes a known safe state (all off, one function-15 write) after every connect and reconnect. | assumption |
| Vendor command table | Three rows have a wrong LRC: channel 15 off prints FF (should be EF), channel 16 on prints FF (should be EF), channel 16 off prints FE (should be EE). Modbus ASCII LRC is the two's complement of the byte sum. | fact (as recorded by the project) |
| Driver consequence | radioctl computes every frame, never copies one from the table. | fact |
| USB identity | CH340 bridge, `1a86:7523`, no serial number. Two identical boards (or other CH340 adapters) cannot be told apart. The port can move after a replug. | fact |
| Isolation and rating | Listings say 16 opto-isolated SPDT relays, 10 A at 250 V, 9-36 V DC supply. Taken from reseller listings, not from a vendor page we could read. | unconfirmed |
| Vendor page | [sainsmart.com product page](https://sainsmart.com/products/16-channel-9-36v-usb-relay-module); it returned HTTP 429 when we fetched it. | unconfirmed |
| Protocol | Modbus ASCII, 9600 8N1, slave `FE`. The gist above calls it "appears to be" Modbus ASCII. | third party |

How radioctl identifies serial ports:

- A target given as a USB matcher (`UsbMatcher`) is looked up again by USB id on every reopen (`crates/radioctl-core/src/serial.rs`, `usb.rs`).
- A matcher on `1a86:7523` alone matches every CH340 adapter on the machine.
- With no serial number there is nothing to tell two boards apart; the station owner must keep exactly one.

Consequence in radioctl:

- A channel on this board can never report a confirmed state.
- Station actions that require confirmation (amplifier power-on, operate) refuse unless an opt-in is added. No opt-in exists yet.
- Use it only for low-consequence outputs, never for anything in the RF or amplifier path.
- Nothing is wired to the board today.
- Its entry in `data/station/boards.toml` carries `recommendation = "not-recommended"`.
- Design details: [manual chapter 14](manual/14-station.md#relay-boards-whose-status-is-not-readable-sainsmart-16-channel).

## 3. Recommended and acceptable (provisional)

All entries: meets the criteria on paper, not yet tested by us. Verified by us on hardware: **no**.

| Role | Product | Status |
|---|---|---|
| LAN switch | Shelly Plus 1 (Gen2) | Recommended |
| Open-firmware relay | Tasmota-flashed relay | Acceptable |
| Home Assistant switch | any HA `switch` entity | Acceptable |
| Modbus RTU relay (RS485) | Waveshare Modbus RTU Relay (8 ch) | Acceptable |
| Modbus RTU relay (RS485) | Waveshare Modbus RTU Relay 16CH | Acceptable |
| Raspberry Pi GPIO relay | Waveshare RPi Relay Board (3 ch) | Acceptable, low-consequence only |

### Shelly Plus 1 (Gen2): Recommended

- Read-back: `Switch.GetStatus` returns `output` (boolean) ([Switch docs](https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Switch)).
- Power-up state: config `initial_state` = `off`, `on`, `restore_last` or `match_input`. Set it to `off` ([Switch docs](https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Switch)).
- Failsafe: `auto_off` with `auto_off_delay` (seconds); status shows `timer_started_at` and `timer_duration` while a timer runs ([Switch docs](https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Switch)).
- Local API: HTTP RPC, for example `GET /rpc/Switch.Set?id=0&on=true` ([Switch docs](https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Switch)).
- Cloud: can work standalone on a local Wi-Fi network; cloud is optional ([Shelly KB](https://kb.shelly.cloud/knowledge-base/shelly-plus-1)).
- Rating: 16 A AC / 10 A DC; 240 V AC / 30 V DC; potential-free contact ([Shelly KB](https://kb.shelly.cloud/knowledge-base/shelly-plus-1)).
- radioctl-station: supported by its Gen2 RPC driver (board `shelly-gen2-1ch`).
- Gaps: Wi-Fi only (no wired); firmware is closed; whether the auto-off timer survives a reboot is unconfirmed; the Pro 1 was not checked.

### Tasmota-flashed relay: Acceptable

- Open firmware; the product depends on the hardware you flash, so pick one with an isolated relay rated for the load. We name no specific device.
- Read-back: `Power` without a parameter returns the state; HTTP form `http://<ip>/cm?cmnd=Power` ([Tasmota commands](https://tasmota.github.io/docs/Commands/)).
- Power-up state: `PowerOnState` 0 = off. The default is 3 (restore last saved state), so it must be changed ([Tasmota commands](https://tasmota.github.io/docs/Commands/)).
- Failsafe: `PulseTime<x>` turns a relay off after a set time following `Power<x> ON` ([Tasmota commands](https://tasmota.github.io/docs/Commands/)).
- Local control: the commands page describes direct HTTP, console and serial access and mentions no cloud requirement.
- Web UI password travels in plain text in the HTTP query ([Tasmota commands](https://tasmota.github.io/docs/Commands/)): private LAN only.
- radioctl-station: Tasmota HTTP driver (board `tasmota-4ch`, `Power1` to `Power4`).
- Unconfirmed: whether `PulseTime` is re-armed after a Wi-Fi drop or reboot.

### Home Assistant switch: Acceptable

- A `switch` entity has states `on`, `off`, `unavailable`, `unknown` and actions `turn_on`, `turn_off`, `toggle` ([HA switch](https://www.home-assistant.io/integrations/switch/)).
- radioctl-station has a Home Assistant driver (private address, plain HTTP and WebSocket).
- Safety is only as good as the integration under the entity: power-up state, failsafe and read-back are the device's, not Home Assistant's.
- Unconfirmed: which integrations report real device state rather than an assumed one; the HA page does not say.
- Prefer the device's own driver (Shelly, Tasmota) when radioctl has one.

### Waveshare Modbus RTU Relay (8 ch) and 16CH: Acceptable

- Read-back: function 0x01 reads coils; 0x05 and 0x0F write ([8 ch wiki](https://www.waveshare.com/wiki/Modbus_RTU_Relay), [16CH wiki](https://www.waveshare.com/wiki/Modbus_RTU_Relay_16CH)).
- Register map is public: relays at 0x0000 (8 ch: to 0x0007; 16CH: to 0x000F), address at 0x4000, UART at 0x2000, version at 0x8000.
- Timed behaviour: "flash on" (0x0200..) and "flash off" (0x0400..) one-shot delays in 100 ms units. These are not a watchdog.
- Failsafe or watchdog: none documented.
- Power-up state: not stated on either wiki; the wiki advises confirming relay status after power-on. radioctl must force a safe state on connect, then read back to confirm.
- Rating and isolation: not stated on either wiki page. A reseller title says "multiple isolation protection circuits"; unconfirmed.
- Interface: RS485 (needs a USB-RS485 adapter with a stable identity); RTU framing exists in `crates/radioctl-station/src/modbus.rs`, but `boards.toml` has no entry for these boards yet.
- The wiki's parity tables conflict; check on hardware.
- Why only Acceptable: no failsafe, power-up state unknown, isolation and rating unconfirmed. Still better than the SainSmart because it can report state.

### Waveshare RPi Relay Board (3 ch): Acceptable, low-consequence only

- Three relays on BCM 26, 20, 21 (pins 37, 38, 40), each NO/NC; low-active ([wiki](https://www.waveshare.com/wiki/RPi_Relay_Board)).
- Read-back: no relay feedback pin documented; reading the GPIO shows what the Pi drives, not the contact.
- Power-up state: "open by default" per the FAQ, after web control starts; the Relay_JMP jumper must be fitted for the Pi to control it.
- Failsafe: none in hardware; GPIO lines float or reset when the Pi stops or reboots.
- Rating and isolation: not stated on the wiki page.
- Raspberry Pi 5 compatibility: not stated.
- Not in `data/station/boards.toml`; add an entry (with pins) before using it with a GPIO map.

### Roles with no entry yet

- Antenna switch and amplifier PTT or power sequencer: the radioctl-station docs name no product, so none is listed.
- A product will be listed here only after a source supports the criteria.

## 4. How to add a product

Checklist, one line per criterion, each with a source URL or "unconfirmed":

- [ ] State read-back
- [ ] Defined, settable power-up state
- [ ] Watchdog, auto-off or failsafe timer
- [ ] Local control with no cloud account
- [ ] Public protocol documentation
- [ ] Isolation and ratings for the load
- [ ] Stable USB identity or network address
- [ ] Price and availability
- [ ] Open firmware (where relevant)

Reports from volunteers:

- Go through the same by-hand issue process as rig verification: see [VOLUNTEER_TESTING.md](VOLUNTEER_TESTING.md).
- A hardware report records product, firmware, date and what was tested.
- Until a report is accepted, the entry says "verified by us on hardware: no".
