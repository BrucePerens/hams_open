# Data structures

### `_Chan` [@ANCHOR: hardware:hardware_fx2_simulator_chan]

**Purpose and location.** The class has no docstring; from its definition it provides `__init__`. Defined in `hardware/fx2_station_io/tools/fx2_simulator.py:34` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`.

**See also:** test `test_keying_a_keyline_or_an_unmapped_pin_is_refused (fx2_station_io/tests/test_fx2_ptt.py)`.

### `SimulatedStationEngine` [@ANCHOR: hardware:hardware_fx2_simulator_simulated_station_engine]

**Purpose and location.** The function-map engine written from the protocol description (virtual clock, 1 ms steps). Defined in `hardware/fx2_station_io/tools/fx2_simulator.py:52` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `reset_all`, `_set`, `_active`, `_safe`, `_primary`, `keyed_mask`, `active_mask`, `_release_channel`, `release_all`, `_fail`, `map_begin`, `map_pin`, `map_param`, `map_heartbeat`, `map_commit`, `_refuse`, `key`, `heartbeat`, `step`, `_tick`, `bus_reset`, `suspend`, `_remaining`, `status`.

**See also:** test `test_parses_simulator_record (fx2_station_io/tests/test_fx2_host.py)`.

### `SimulatedFx2` [@ANCHOR: hardware:hardware_fx2_simulator_simulated_fx2]

**Purpose and location.** State: ROM (unprogrammed) or RUNNING; RAM; port A output-enable and latch. Defined in `hardware/fx2_station_io/tools/fx2_simulator.py:328` as a class; bases: `Transport`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `ctrl_out`, `sleep`, `bus_reset`, `suspend`, `_release_manual`, `_pa`, `ctrl_in`, `ram_loaded`.

**See also:** test `test_load_holds_resets_writes_verifies_releases_only_via_a0 (fx2_station_io/tests/test_fx2_host.py)`.

### `Fx2Error` [@ANCHOR: hardware:hardware_hams_fx2_fx2_error]

**Purpose and location.** A protocol, safety or hardware problem; the message says what to do. Defined in `hardware/fx2_station_io/tools/hams_fx2.py:60` as a class; bases: `Exception`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_refuses_unknown_major_version (fx2_station_io/tests/test_fx2_host.py)`.

### `Stalled` [@ANCHOR: hardware:hardware_hams_fx2_stalled]

**Purpose and location.** The device answered a control request with a STALL (it refused the request). Defined in `hardware/fx2_station_io/tools/hams_fx2.py:64` as a class; bases: `Fx2Error`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_map_refuses_parameters_on_non_ptt_pins_and_steps_out_of_order (fx2_station_io/tests/test_fx2_ptt.py)`.

### `Transport` [@ANCHOR: hardware:hardware_hams_fx2_transport]

**Purpose and location.** Control-transfer access to one device. Defined in `hardware/fx2_station_io/tools/hams_fx2.py:68` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `sleep`, `ctrl_in`, `ctrl_out`.

**See also:** test `test_readback_mismatch_leaves_cpu_in_reset (fx2_station_io/tests/test_fx2_host.py)`.

### `GetInfo` [@ANCHOR: hardware:hardware_hams_fx2_get_info]

**Purpose and location.** Parsed GET_INFO record, protocol 0.1 (layout in README.md). Defined in `hardware/fx2_station_io/tools/hams_fx2.py:83` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `capability_names`.

**See also:** test `test_parses_simulator_record (fx2_station_io/tests/test_fx2_host.py)`.

### `Tick` [@ANCHOR: hardware:hardware_hams_fx2_tick]

**Purpose and location.** The firmware's millisecond clock (Timer 2) and worst-case main-loop latency (Timer 0). Defined in `hardware/fx2_station_io/tools/hams_fx2.py:227` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `seconds`, `loop_max_us`.

**See also:** test `test_tick_advances_at_one_millisecond_per_count (fx2_station_io/tests/test_fx2_host.py)`.

### `Status` [@ANCHOR: hardware:hardware_hams_fx2_status]

**Purpose and location.** The status record (README, "Status record"). Defined in `hardware/fx2_station_io/tools/hams_fx2.py:276` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `reason_name`.

**See also:** test `test_usb_suspend_releases_the_line (fx2_station_io/tests/test_fx2_ptt.py)`.

### `PinSpec` [@ANCHOR: hardware:hardware_hams_fx2_pin_spec]

**Purpose and location.** One map entry: a pin, its function, safe state and timers. Defined in `hardware/fx2_station_io/tools/hams_fx2.py:317` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_map_refuses_two_functions_on_one_pin (fx2_station_io/tests/test_fx2_ptt.py)`.

### `UsbTransport` [@ANCHOR: hardware:hardware_hams_fx2_usb_transport]

**Purpose and location.** pyusb transport for one device. Control transfers only; no interface is claimed. Defined in `hardware/fx2_station_io/tools/hams_fx2.py:406` as a class; bases: `Transport`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `ctrl_in`, `ctrl_out`.

**See also:** test `test_readback_mismatch_leaves_cpu_in_reset (fx2_station_io/tests/test_fx2_host.py)`.

### `Heartbeater` [@ANCHOR: hardware:hardware_hams_fx2_heartbeater]

**Purpose and location.** The host side of the FX2 station-I/O board's heartbeat: it sends sequence-numbered heartbeats that carry the commanded state of every channel, the way the relay's transmit task does. If the heartbeats stop, the board's firmware releases the channels (drops PTT) after the configured time-out, so this is a transmit-safety path on the host. Defined in `hardware/fx2_station_io/tools/hams_fx2.py`; used by the behaviour tests (`tests/test_fx2_ptt.py`) and by the command-line tool.

**Definition.** `class Heartbeater`, built as `Heartbeater(t)`.

| Attribute | Type | Meaning |
|---|---|---|
| `t` | `Transport` | The control-transfer transport the heartbeats go through (`UsbTransport` for a real board, the simulator in tests). |
| `seq` | `int` | The sequence number of the last heartbeat sent, 16 bits; starts at 0. `beat()` adds 1 modulo 65536 before sending, so the first heartbeat carries 1. |
| `commanded` | `int` | The commanded state, one bit per channel (a PTT channel and its key line share a bit); only the low 8 bits are sent. Set it before calling `beat()`; starts at 0. |

**Method.** `beat() -> Status`: increments `seq`, sends one vendor control IN request `REQ_HEARTBEAT` (request type `0xC0`) with `seq` as the value and `commanded` as the index, and returns the board's reply parsed as a `Status` (including `last_seq`, which echoes the sequence number the board saw). A transfer error or a short reply raises `Fx2Error` or `Stalled` from the transport or from `parse_status`.

**Concurrency and safety.** Not thread safe: one thread owns a `Heartbeater` and calls `beat()` at the rate the board's heartbeat time-out requires; the object keeps no clock of its own, so the caller's schedule decides when the board's heartbeat window is met. Real hardware is only touched through `t`.

**Functions that touch it.** `Heartbeater.beat` and `heartbeat`.

**See also:** tests `HeartbeaterTests` in `hardware/fx2_station_io/tests/test_fx2_host.py` (sequence numbers, commanded state, wrap at 16 bits) and `tests/test_fx2_ptt.py` (the whole PTT behaviour, against the simulator and optionally the real board).
