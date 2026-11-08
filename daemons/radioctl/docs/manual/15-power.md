# 15. Transmit power and meters

radioctl reports transmit power in one unit, watts, from one of two sources: the rig's own meter or an
external wattmeter. The rules are design decision 26 ([`../DESIGN.md`](../DESIGN.md)); this chapter says
what the code does. Status: **implemented, unverified on hardware** for every source except the KX3
reading in the table at the end, which has a bench result.

## The raw meters and the calibrated power

A rig's meters arrive as raw deflections that radioctl does not convert, because the conversion is a
measurement of each model (chapter [4](04-api.md#operations)):

- `Op::GetPower` is the transmit power meter, `Observation::Power`, a raw number in the family's own
  scale (a CI-V 0 to 255 deflection, an Elecraft `PO` reading in tenths of a watt).
- `Op::GetMeter(Meter)` reads the SWR, ALC, COMP, Vd (supply voltage) or Id (final-stage current)
  meter as a raw deflection. `radioctld` serves them as `SWR`, `ALC`, `COMP_METER`, `VD_METER` and
  `ID_METER` after its own scaling, which its source marks as an unverified estimate (chapter
  [9](09-radioctld.md)).

A description turns the raw power number into watts only if it carries a `[power_meter]` block: at
least two `[raw, milliwatts]` points, ascending in raw value and not falling in power, with one
`[[facts]]` entry (`power_meter.raw_to_milliwatts`) saying where they came from (chapter
[6](06-rig-descriptions.md)). Between points the watts are interpolated linearly; above the last point
they stay at the last point's value. Two descriptions carry one today, the Elecraft KX3 and the Icom
IC-7851.

```toml fragment
# From rigs/elecraft-kx3.toml: PO is tenths of a watt in QRP mode, so 150 is 15 W.
[power_meter]
raw_to_milliwatts = [[0, 0], [150, 15000]]
```

**A rig without the block never reports watts.** Its reading is a relative bar from 0 to 100,
flagged `relative`, and nothing downstream may call it watts. The KX3's calibration is wrong when a
KXPA100 amplifier is fitted (the unit is then watts); the description's fact says so, and the
self-test's `identify: power amplifier` check, which reads `OM;`, tells the two cases apart
(`../VOLUNTEER_TESTING.md`).

## One unit, converted at the source

The `radioctl_core::power` module holds `Power`, a finite, non-negative number of watts.
Conversions happen where a reading is made, so nothing downstream sees another unit:

| From | Constructor | Formula |
|---|---|---|
| watts | `Power::from_watts` | refuses negative, infinite and not-a-number figures |
| dBm | `Power::from_dbm` | `10^((dBm - 30) / 10)` watts |
| dBW | `Power::from_dbw` | `10^(dBW / 10)` watts |
| dBmV across 50 ohms | `Power::from_dbmv_50ohm` | `V^2 / 50` with `V = 10^(dBmV / 20)` millivolts |

`PowerUnit` names the unit a meter reports in (`w`, `dbm`, `dbw`, `dbmv50`), and
`PowerUnit::to_power(value, offset_db)` converts a figure read behind a coupler and attenuator that lose
`offset_db` decibels (positive: the meter reads lower than the line) into watts at the transmitter. The
offset is applied in the decibel domain: a meter reading 0 dBm behind 30 dB of coupler and attenuator
is 1 W at the transmitter.

A `PowerReading` carries the forward watts, optional reflected watts, its `PowerOrigin` (`Rig`, or
`External` with the meter's name), how long it stays good (`valid_for`), whether it is a relative bar,
and the meter's full scale in watts when known. Anything that can produce one implements the
`RfPowerReader` trait. **A reading that is too old is no power:** `read` returns nothing, never the last
number. The rig's own reading stays good for 1.5 seconds.

## What radioctld serves

`radioctld` answers Hamlib's two power levels, so `rigctl` and the programs built on it work:

| Level | Value | When |
|---|---|---|
| `RFPOWER_METER` | the output power as a fraction, 0 to 1, of the meter's scale | always when the rig reads power; with a `[power_meter]` calibration the fraction of its last point, without one `raw / 255` as a relative bar |
| `RFPOWER_METER_WATTS` | the output power in watts | only with a calibration; a rig without one answers that the command is not available |

The transmit-power *setting* (`RFPOWER`) is a different thing: a request is rounded down, so half of a
15 W scale is 7 W, never 8 W.

## Showing a power

`format_power` renders a reading for a display with three significant digits and a fixed number of
decimals, so the field does not jump: milliwatts below 1 W, watts from 1 W to 999 W, kilowatts from
1000 W (`250 mW`, `12.5 W`, `100 W`, `1.50 kW`), and `--` for no power. A unit switches up at exactly 1 W
and 1000 W and back down only 5 percent below the threshold (0.95 W, 950 W), so a reading that wobbles
around a threshold does not flip the unit. The caller passes the unit it showed last.

## External wattmeters

The `radioctl-station` crate (chapter [14](14-station.md)) has the external sources. The drivers read
the output power of Elecraft amplifiers over a serial link:

| Driver | Reads | Reply |
|---|---|---|
| `kpa500` | forward watts (its SWR field is not used) | `^WS;` answers `^WSppp sss;`, no reflected power |
| `kpa1500` | forward and reflected watts | `^PWF;` and `^PWR;`, one request at a time |

A `[[power_meters]]` entry in the station definition names the meter (`id`), the rig it measures
(`for_rig`, at most one meter per rig), the `driver`, the `port`, and optionally `baud`, `unit`,
`offset_db_tenths` (the coupler and attenuator loss in tenths of a decibel, 0 for the amplifier meters,
which sit inside the amplifier) and `stale_ms` (3000 when absent). `MeterPoller` reads the meter over a
byte stream; `PowerFeed` holds the latest sample and implements `RfPowerReader`, so a stale sample is no
power; a virtual meter serves tests. The definition is validated when the station definition loads.

**Not wired yet.** Nothing in `radioctld` or `radioctl-capi` reads a `PowerFeed`: the external meter is
a library component, with its definition validated, and `RFPOWER_METER_WATTS` still comes from the rig's
own meter. The drivers' facts come from Elecraft's own documents and have not been checked on an
amplifier. [`../RECOMMENDED_HARDWARE.md`](../RECOMMENDED_HARDWARE.md) lists which wattmeters suit and which
do not.

## What a test proves

The conversions, the calibration, the formatter and the unit hysteresis are unit tests in
`radioctl-core/src/power.rs`; the poller's replies are tested against the virtual meter; the KX3
calibration was exercised on the bench (`PO048`, 4.8 W, while sending a test message into a dummy load,
`PO000` in receive; `rigs/elecraft-kx3.toml` records it as `hardware-verified`). The IC-7851 points are
`manual-checked` against Icom's table and have not been run on a radio.
