#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Turns a `bench_console --wire` capture and its `--log` into a conformance transcript.

`bench_console --wire FILE` writes `SECONDS tx|rx HEX` lines (every read and write on the link);
`--log FILE` writes the console's own lines (`SECONDS > setfreq a 7074000`, `SECONDS = ok (1 ms)`,
`SECONDS = Frequency(A, Hz(14074000)) (3 ms)`). This joins them into the `[[step]]` file that
`conformance::replay` reads, with `origin = "capture"`:

* each console command is paired with the last exchange (a `tx` frame and the `rx` bytes up to the
  next `tx`) answered before its `=` result was printed and started after its `>` line: the actor
  serves one request at a time and prints the result as soon as the answer is in, so the command's
  own request is the last one to finish before that line, and the polls are the others. When more
  than one exchange ran in that window the step carries a `# check:` comment;
* the step's `op` comes from the command, its `expect` from the result (`ok` is an `ack`);
* a command or result this tool does not know is left out with a `# not converted` comment, and
  so are the exchanges nobody asked for (the actor's own polls) and `rx` bytes with no request
  before them (unsolicited frames from the front panel): they are listed as comments, because
  turning them into `panel` steps needs a decode only a person can judge;
* `--redact HEX` (repeatable, like `fe fe 94 e0`-style byte strings) replaces those bytes with
  zeros of the same length in every frame, for serial numbers and addresses.

The result is a starting point: read it, scrub it, and check it replays (copy it to
`rigs/transcripts/` and run `cargo test -p radioctl-protocols --test rig_files`) before keeping it.

Usage:
    wire_to_transcript.py --rig ID --wire FILE --log FILE [--redact HEX]... [--out FILE]
"""
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

WIRE_LINE = re.compile(r"^\s*(\d+\.\d+)\s+(tx|rx)\s*((?:[0-9a-fA-F]{2}\s*)*)$")
LOG_LINE = re.compile(r"^\s*(\d+\.\d+)\s+(.*)$")
FREQ_RESULT = re.compile(r"^Frequency\((A|B), Hz\((\d+)\)\)")


@dataclass
class Frame:
    time: float
    direction: str
    data: bytes


@dataclass
class Command:
    time: float
    text: str
    done: float | None = None
    result: str = ""


@dataclass
class Exchange:
    """One `tx` frame and the `rx` bytes that followed it before the next `tx`."""

    time: float
    tx: bytes
    rx: bytes = b""
    rx_time: float = 0.0
    owner: Command | None = None


# [@ANCHOR: radioctl_wire_parse_wire]
def parse_wire(text: str) -> list[Frame]:
    """The `SECONDS tx|rx HEX` lines of a `--wire` file; any other line is an error."""
    frames = []
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        match = WIRE_LINE.match(line)
        if not match:
            raise ValueError(f"wire line {number} is not 'SECONDS tx|rx HEX': {line!r}")
        frames.append(Frame(float(match[1]), match[2], bytes.fromhex(match[3].replace(" ", ""))))
    return frames


# [@ANCHOR: radioctl_wire_parse_log]
def parse_log(text: str) -> list[Command]:
    """The commands (`> ...`) of a `--log` file with the result line (`= ...`) that ended each.

    Event lines, notes and the final stats are ignored. A command with no result line before the
    next command keeps `done = None` and is not converted."""
    commands: list[Command] = []
    for line in text.splitlines():
        match = LOG_LINE.match(line)
        if not match:
            continue
        time, rest = float(match[1]), match[2]
        if rest.startswith("> "):
            commands.append(Command(time, rest[2:].strip()))
        elif rest.startswith("= ") and commands and commands[-1].done is None:
            commands[-1].done = time
            commands[-1].result = re.sub(r"\s*\(\d+ ms\)$", "", rest[2:].strip())
    return commands


# [@ANCHOR: radioctl_wire_exchanges]
def exchanges(frames: list[Frame]) -> tuple[list[Exchange], list[Frame]]:
    """Pairs each `tx` frame with the `rx` bytes after it; `rx` before any `tx` is unsolicited."""
    out: list[Exchange] = []
    stray: list[Frame] = []
    for frame in frames:
        if frame.direction == "tx":
            out.append(Exchange(frame.time, frame.data))
        elif out:
            out[-1].rx += frame.data
            out[-1].rx_time = frame.time
        else:
            stray.append(frame)
    return out, stray


# [@ANCHOR: radioctl_wire_op_of]
def op_of(command: str) -> str | None:
    """The transcript `op` table for a console command, or None if this tool does not know it."""
    words = command.split()
    if not words:
        return None
    name, args = words[0].lower(), words[1:]
    vfo = (args[0].lower() if args and args[0].lower() in ("a", "b") else "a")
    simple = {
        "mode": '{ kind = "get-mode" }',
        "signal": '{ kind = "get-signal" }',
        "split": '{ kind = "get-split" }',
        "rit": '{ kind = "get-rit" }',
        "ritoffset": '{ kind = "get-rit-offset" }',
        "filter": '{ kind = "get-filter-width" }',
    }
    if name in simple and not args:
        return simple[name]
    if name == "freq" and len(args) <= 1:
        return f'{{ kind = "get-frequency", vfo = "{vfo}" }}'
    if name == "setfreq" and len(args) == 2 and args[0].lower() in ("a", "b") and args[1].isdigit():
        return f'{{ kind = "set-frequency", vfo = "{args[0].lower()}", hz = {int(args[1])} }}'
    if name == "setmode" and len(args) == 1 and re.fullmatch(r"[A-Za-z0-9-]+", args[0]):
        return f'{{ kind = "set-mode", mode = "{args[0].lower()}" }}'
    if name == "setsplit" and len(args) == 1 and args[0] in ("0", "1"):
        return f'{{ kind = "set-split", on = {"true" if args[0] == "1" else "false"} }}'
    if name == "setritoffset" and len(args) == 1 and re.fullmatch(r"-?\d+", args[0]):
        return f'{{ kind = "set-rit-offset", hz = {int(args[0])} }}'
    if name == "setfilter" and len(args) == 1 and args[0].isdigit():
        return f'{{ kind = "set-filter-width", hz = {int(args[0])} }}'
    return None


# [@ANCHOR: radioctl_wire_expect_of]
def expect_of(result: str) -> str | None:
    """The transcript `expect` table for a console result line, or None if unknown."""
    if result == "ok":
        return '{ kind = "ack" }'
    freq = FREQ_RESULT.match(result)
    if freq:
        return f'{{ kind = "frequency", vfo = "{freq[1].lower()}", hz = {int(freq[2])} }}'
    mode = re.fullmatch(r"Mode\((\w+)\)", result)
    if mode:
        return f'{{ kind = "mode", mode = "{mode[1].lower()}" }}'
    signal = re.fullmatch(r"Signal\((\d+)\)", result)
    if signal:
        return f'{{ kind = "signal", value = {int(signal[1])} }}'
    split = re.fullmatch(r"Split\((true|false)\)", result)
    if split:
        return f'{{ kind = "split", on = {split[1]} }}'
    return None


# [@ANCHOR: radioctl_wire_redact]
def redact(data: bytes, patterns: list[bytes]) -> bytes:
    """Replaces every occurrence of each pattern with zero bytes of the same length."""
    for pattern in patterns:
        if pattern:
            data = data.replace(pattern, bytes(len(pattern)))
    return data


# [@ANCHOR: radioctl_wire_toml_text]
def toml_text(data: bytes) -> str | None:
    """A TOML basic string for ASCII text (`\\r` and `\\n` escaped), or None for binary bytes."""
    if not data or any(not (0x20 <= b < 0x7F or b in (0x0D, 0x0A)) for b in data):
        return None
    text = data.decode("ascii").replace("\\", "\\\\").replace('"', '\\"')
    return '"' + text.replace("\r", "\\r").replace("\n", "\\n") + '"'


def hexs(data: bytes) -> str:
    return " ".join(f"{b:02x}" for b in data)


# [@ANCHOR: radioctl_wire_convert]
def convert(rig: str, wire: str, log: str, redactions: list[bytes] | None = None) -> str:
    """The transcript TOML for one capture."""
    redactions = redactions or []
    commands = parse_log(log)
    pairs, stray = exchanges(parse_wire(wire))
    checks: dict[int, int] = {}
    for command in commands:
        if command.done is None:
            continue
        window = [p for p in pairs
                  if p.owner is None and command.time - 0.0005 <= p.time and p.rx_time <= command.done + 0.0005]
        if window:
            window[-1].owner = command
            checks[id(command)] = len(window)
    steps: list[str] = []
    notes: list[str] = []
    for command in commands:
        pair = next((p for p in pairs if p.owner is command), None)
        op = op_of(command.text)
        expect = expect_of(command.result) if command.done is not None else None
        if pair is None or not pair.rx or op is None or expect is None:
            why = "no result" if command.done is None else "no frame" if pair is None or not pair.rx else (
                "unknown command" if op is None else "unknown result")
            notes.append(f"# not converted ({why}): > {command.text} = {command.result}")
            continue
        tx, rx = redact(pair.tx, redactions), redact(pair.rx, redactions)
        tx_text, rx_text = toml_text(tx), toml_text(rx)
        lines = ["[[step]]"]
        if checks.get(id(command), 1) > 1:
            lines.append(f"# check: {checks[id(command)]} exchanges ran while '{command.text}' did; this is the last to finish")
        lines.append(f"op = {op}")
        lines.append(f"tx_text = {tx_text}" if tx_text else f'tx = "{hexs(tx)}"')
        lines.append(f"rx_text = {rx_text}" if rx_text else f'rx = "{hexs(rx)}"')
        lines.append(f"expect = {expect}")
        steps.append("\n".join(lines))
    for pair in pairs:
        if pair.owner is None:
            notes.append(f"# unattributed exchange at {pair.time:.3f} s (a poll?): "
                         f"tx {hexs(redact(pair.tx, redactions))} rx {hexs(redact(pair.rx, redactions))}")
    for frame in stray:
        notes.append(f"# unsolicited rx at {frame.time:.3f} s: {hexs(redact(frame.data, redactions))}")
    if not steps:
        raise ValueError("no console command could be paired with a frame and converted")
    header = [
        f"# Conformance transcript for rigs/{rig}.toml, converted by tools/wire_to_transcript.py from a",
        "# bench_console --wire capture and its --log. Read it and scrub serial numbers before keeping it.",
        "schema = 1",
        f'rig = "{rig}"',
        'origin = "capture"',
    ]
    return "\n".join(header) + "\n\n" + "\n\n".join(steps) + "\n\n" + "\n".join(notes) + "\n"


# [@ANCHOR: radioctl_wire_main]
def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--rig", required=True, help="the rig id (a file in rigs/)")
    parser.add_argument("--wire", required=True, type=Path)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--redact", action="append", default=[], help="hex bytes to zero, e.g. 'de ad be ef'")
    parser.add_argument("--out", type=Path, help="write here instead of standard output")
    args = parser.parse_args(argv)
    try:
        redactions = [bytes.fromhex(r.replace(" ", "")) for r in args.redact]
        text = convert(args.rig, args.wire.read_text(), args.log.read_text(), redactions)
    except (ValueError, OSError) as error:
        print(f"wire_to_transcript: {error}", file=sys.stderr)
        return 1
    if args.out:
        args.out.write_text(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
