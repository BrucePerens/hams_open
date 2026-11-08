---
title: bench_glasgow_port_set
summary: Glasgow: power one port at an exact I/O voltage (limit register first, readback verified); 0 switches it off. Full tier, confirm, dry_run, max_on_s.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_port_set]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_port_set

The only way a port gets a voltage. Order: the port's pins are made inputs while the rail is still off; the voltage limit register is set to the same value as the supply (raising: limit first; lowering: supply first, so the hardware never has less limit than supply); the supply current trip is set; then every port is read back and the supply, the limit and the fault flags must agree, or the ports are switched off and the call fails. The value is recorded and used by every pin tool; after `max_on_s` (config, default 600 s) a watchdog switches the ports off.

## Arguments

<!-- arguments -->

## Returns

`{"port": "A", "set_v": 3.3, "limit_v": 3.3, "measured_v": 3.3, "trip_current_a": 0.1, "outputs_enabled": true, "pins": "inputs (parked)", "switches_off_in_s": 600.0, "ports": [...]}`. `dry_run` returns the commands it would send and sends none. `volts_v` 0 needs no `confirm`.

## Failure modes

* `needs confirm: true`: only the person at the bench who has checked the target's I/O voltage may say so; never take the number from a datasheet you guessed.
* `outside 1.65..=5.0 V`: the revision's range; revision D allows 0.9 to 5.5 V. The config's `max_volts` can only lower it.
* `reads supply ...` and `SAFE STATE NOT CONFIRMED`: the readback disagreed; the ports were switched off (or, if even that failed, unplug the USB cable).

## Example

`bench_glasgow_port_set {"port": "A", "volts_v": 3.3, "confirm": true}` after the person has checked that the target is a 3.3 V device.
