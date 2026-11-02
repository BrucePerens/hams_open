# Appendix C. radioctld command reference

The `rigctld` text protocol as radioctld serves it. The source of truth is
`crates/radioctld/src/command.rs` (parser), `service.rs` (behaviour) and `reply.rs` (rendering); the
recorded sessions in `crates/radioctld/tests/transcripts/` show exact bytes. Chapter [9](09-radioctld.md)
explains the design and the differences from Hamlib's `rigctld`.

## Request and response format

- A request is one line ending in a newline. `+` before the command asks for the **extended**
  response format. A command is a one-letter form or `\long_name`. Arguments are separated by blanks.
  Lines over `max_line_bytes`, or with control characters, are `RPRT -1`.
- **Plain** responses: a get prints its values one per line; a set prints `RPRT 0`; a failure prints
  `RPRT <code>`.
- **Extended** responses: a header line `long_name: arguments`, then `Label: value` lines, then
  `RPRT <code>`.

```text
> +f
< get_freq:
< Frequency: 14060000
< RPRT 0
```

- Frequencies are integer hertz in responses; `F` also accepts a decimal such as `7074000.000000`.
- Every command that takes a VFO accepts an optional leading VFO token (`VFOA`, `VFOB`, `currVFO`,
  `Main`, `MainA`, `MainB`, `RX`, `TX`) in either mode. `RX` and `TX` are the receive and transmit VFOs from the rig's last reports (chapter 4).

## Error codes

| Code | Name | When radioctld answers it |
|---|---|---|
| 0 | | success |
| -1 | invalid parameter | missing, surplus or malformed argument; unknown command word; unknown mode, level or function name |
| -4 | not implemented | a Hamlib command radioctld knows by name and does not implement |
| -5 | timeout | the rig did not answer in time |
| -6 | I/O error | the link is down, or the command was cancelled |
| -8 | protocol error | the rig said something the engine could not use |
| -9 | command rejected | the rig refused; **or** a transmit command while the gate is closed, or another client owns the transmitter |
| -11 | feature not available | the rig or its description lacks it; raw `send_cmd`, power control; a VFO the rig has not |
| -14 | bus busy | the rig's command queue is full |

An `ErrorKind` maps as: `Rejected` and `TransmitGateClosed` to -9, `Unsupported` to -11, `LinkDown` and
`Cancelled` to -6, `Timeout` to -5, `Malformed` to -8, `QueueFull` to -14 (`reply::error_code`).

## Commands served

| Short | Long name | Arguments | Response (extended labels) | Notes |
|---|---|---|---|---|
| `f` | `get_freq` | [vfo] | `Frequency` | Hz. Answers from the cache if newer than `cache_max_age_ms`. |
| `F` | `set_freq` | [vfo] freq | | |
| `m` | `get_mode` | [vfo] | `Mode`, `Passband` | Hamlib mode names: `AM CW USB LSB RTTY FM WFM CWR RTTYR PKTLSB PKTUSB`. The two data modes are `PKTUSB` and `PKTLSB`. The passband is the mode's typical width when the rig has no filter control. |
| `M` | `set_mode` | [vfo] mode passband | | A passband of 0 or -1 keeps the rig's own. |
| `v` | `get_vfo` | | `VFO` | radioctld's own selection, not the radio's. |
| `V` | `set_vfo` | vfo | | `VFOB` needs `second-vfo`. |
| `t` | `get_ptt` | [vfo] | `PTT` | 0 or 1. |
| `T` | `set_ptt` | [vfo] 0\|1\|2\|3 | | 0 unkeys, always allowed. Any other value keys: needs the gate open and the link up, and the transmitter not owned by another client. |
| `s` | `get_split_vfo` | [vfo] | `Split`, `TX VFO` | The transmit VFO is `VFOB` when split is on, else `VFOA`. |
| `S` | `set_split_vfo` | [vfo] split txvfo | | Split on needs `VFOB` as the transmit VFO and the `split` capability. |
| `i` | `get_split_freq` | [vfo] | `TX Frequency` | Always VFO B. |
| `I` | `set_split_freq` | [vfo] freq | | Sets VFO B. |
| `x` | `get_split_mode` | [vfo] | `TX Mode`, `TX Passband` | The rig's mode. |
| `X` | `set_split_mode` | [vfo] mode passband | | Only the mode the rig is already in; others `RPRT -11`. |
| | `get_active_tx_freq` | | `Active TX Frequency` | radioctld's own command (not in Hamlib): the frequency the rig transmits on now or when keyed, whichever VFO is current. `f` keeps Hamlib's meaning, the frequency of the VFO asked for. |
| | `get_split_freq_mode` | [vfo] | `TX Frequency`, `TX Mode`, `TX Passband` | |
| | `set_split_freq_mode` | [vfo] freq mode passband | | Same mode rule as `X`. |
| `j` | `get_rit` | [vfo] | `RIT` | The offset in Hz while RIT is on, else 0. |
| `J` | `set_rit` | [vfo] offset | | 0 switches RIT off; any other offset sets it and switches RIT on. |
| `z` | `get_xit` | [vfo] | `XIT` | |
| `Z` | `set_xit` | [vfo] offset | | Likewise for XIT. |
| `l` | `get_level` | [vfo] name | value only | Names: `AF`, `RF`, `RFPOWER`, `MICGAIN`, `KEYSPD`, `MONITOR_GAIN`, `RAWSTR`, `ALC`, `SWR`, `COMP_METER`, `VD_METER`, `ID_METER`, `ATT`, `PREAMP` (those the rig has). Levels are 0 to 1 floats, except `KEYSPD` (words per minute), `RAWSTR` (raw), `SWR`, `ATT` (integer decibels; only the values in the rig's `Attenuator` list) and `PREAMP` (an integer step, 0 off; Hamlib's own is decibels). `SWR` needs the rig's `read-swr` (a calibrated ratio) or `read-meters`. `ALC` and, without `read-swr`, `SWR` are answered from the CI-V rigs' raw 0 to 255 meter deflections through `meter_cal`, which is **unverified**: the points are those printed in one Icom model's manual (SWR 0 = 1.0, 48 = 1.5, 80 = 2.0, 120 = 3.0, then the last slope up to 10.0; ALC 0 to 120 = 0 to 1), other models print other points, and no radio has checked them. The meters read only while the rig is keyed (by other means; the read never keys it), so unkeyed they answer `1` and `0`. The library's `GetMeter` and `bench_console`'s `meter` give the raw value. `COMP_METER` (decibels of compression, 0 to 30), `VD_METER` (volts, 0 to 16) and `ID_METER` (amperes, 0 to 25) are answered the same way from the Comp, Vd and Id meters, with the scale points Hamlib 4.6.2 carries for the IC-7300 (read for facts; the numbers are those points); beyond the last point the value stays at the last, as Hamlib's does; they are read-only (`L` is `RPRT -11`) and equally **unverified**. The raw deflection (an integer, 0 to 255) is what the library's `GetMeter` returns; Hamlib's floats exist only here, in `radioctld`. `COMP` (the speech-compressor level) is not the meter and stays `RPRT -11`. |
| `L` | `set_level` | [vfo] name value | | An out-of-range value, or an `ATT` or `PREAMP` value the rig has no step for, is `RPRT -1`. |
| `u` | `get_func` | [vfo] name | value only | 0 or 1. Functions served: `RIT`, `XIT`. |
| `U` | `set_func` | [vfo] name 0\|1 | | |
| `b` | `send_morse` | text | | At most 64 characters. Transmits: gate, link and ownership rules as `T 1`, and it claims the transmitter the same way. |
| | `stop_morse` | | | Stops Morse in progress (clears the rig's CW buffer). A stop path: open to every client, owner or not, gate open or closed, and it jumps the rig's queue like `T 0`. It does not unkey and does not change the owner. `RPRT -11` on a rig without a stop command (Kenwood, Yaesu: their descriptions have no `send-cw`). |
| `1` | `dump_caps` | | free text, then `RPRT 0` | |
| | `dump_state` | | free text | Generated from the description. A transmit range is advertised only while the gate is open. |
| | `chk_vfo` | | `ChkVFO` | 1 in `vfo_mode`, else 0. |
| | `get_powerstat` | | `Power Status` | 1 while the link is up. |
| | `get_info`, `get_rig_info` | | `Info` | The rig's display name. |
| | `get_lock_mode` | | `Locked` | Always 0. |
| | `set_lock_mode` | 0\|1 | | 0 accepted; 1 is `RPRT -11`. |
| | `get_vfo_info` | [vfo] | `Freq`, `Mode`, `Width`, `Split`, `SatMode` | |
| | `get_vfo_list` | | `VFOs` | `VFOA VFOB`, or `VFOA`. |
| `q`, `Q` | `quit` | | `RPRT 0`, then the connection closes | |

## Commands never served

| Command | Answer | Why |
|---|---|---|
| `w`, `send_cmd` | `RPRT -11` | A raw escape hatch to the radio must be an explicit, separately named capability. |
| `set_powerstat` | `RPRT -11` | Power control is not implemented. |
| `get_dcd`, memories (`get_mem`, `set_mem`, `get_channel`, `set_channel`), `scan`, `vfo_op`, CTCSS, DCS, repeater shift and offset, antenna, tuning step, parameters, DTMF, transceive, `reset` and the other one-letter Hamlib forms | `RPRT -4` | Known to Hamlib, not implemented. |
| any other word | `RPRT -1` | |

## Session examples

A recorded session of the virtual KX3 (from `tests/transcripts/kx3.txt`; `>` is the client, `<` the
server):

```text
> F 14074000
< RPRT 0
> f
< 14074000
> M USB 2400
< RPRT 0
> +m
< get_mode:
< Mode: USB
< Passband: 2400
< RPRT 0
> S 1 VFOB
< RPRT 0
> s
< 1
< VFOB
> w FA;
< RPRT -11
> \no_such_command
< RPRT -1
```
