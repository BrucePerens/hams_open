# SPDX-License-Identifier: AGPL-3.0-or-later
# The helper script of the bench_glasgow_* tools. bench_mcp runs it with the pinned install's own command line:
#     glasgow script ops.py <applet> <applet options> -- <op> '<json payload>'
# Glasgow evaluates a one-line entry script (written by bench_mcp next to this file) inside the applet's context; the entry loads this module
# and calls `main(globals())`, so `ns` below holds `args` (the parsed command line), `device`, `self` and the applet's interface object
# (`i2c_iface`, `uart_iface`, `gpio_iface`, `swd_iface`). The helper prints exactly one line,
# `BENCH_GLASGOW_RESULT {json}`; a script that raises, or that ends without printing it, is a failed call (the Rust side checks).
# Nothing here reads a file, opens a socket or calls another program. Every wait is bounded by the payload's own limits.
import asyncio
import json
import time


async def _i2c_scan(ns, p):
    # [@ANCHOR: glasgow_ops_i2c_scan]
    i2c_iface = ns["i2c_iface"]
    found = []
    for addr in range(p["lo"], p["hi"] + 1):
        if await i2c_iface.ping(addr):
            found.append(addr)
    return {"addresses": found, "addresses_hex": ["0x%02x" % a for a in found], "range": ["0x%02x" % p["lo"], "0x%02x" % p["hi"]]}


async def _i2c_read(ns, p):
    # [@ANCHOR: glasgow_ops_i2c_read]
    i2c_iface = ns["i2c_iface"]
    try:
        async with i2c_iface.transaction():
            if p["register"]:
                await i2c_iface.write(p["address"], bytes(p["register"]))
            data = await i2c_iface.read(p["address"], p["count"])
    except Exception as e:  # I2CNotAcknowledged and bus errors are results of the call, not crashes
        return {"acknowledged": False, "error": type(e).__name__ + ": " + str(e)}
    return {"acknowledged": True, "hex": bytes(data).hex(), "count": len(data)}


async def _i2c_write(ns, p):
    # [@ANCHOR: glasgow_ops_i2c_write]
    i2c_iface = ns["i2c_iface"]
    try:
        await i2c_iface.write(p["address"], bytes(p["data"]))
    except Exception as e:
        return {"acknowledged": False, "error": type(e).__name__ + ": " + str(e)}
    return {"acknowledged": True, "written": len(p["data"])}


async def _uart_transact(ns, p):
    # [@ANCHOR: glasgow_ops_uart_transact]
    uart_iface = ns["uart_iface"]
    t0 = time.monotonic()
    if p["send"]:
        await uart_iface.write(bytes(p["send"]), flush=True)
    got = bytearray()
    until = bytes(p["until"])
    deadline = t0 + p["timeout_s"]
    last = None
    ended = "timeout"
    while True:
        now = time.monotonic()
        remaining = deadline - now
        idle_left = None if last is None else p["idle_ms"] / 1000.0 - (now - last)
        # The idle limit ends the call only when it comes before the deadline: a device still sending at the deadline did not go quiet.
        idle_first = idle_left is not None and idle_left < remaining
        wait = idle_left if idle_first else remaining
        if wait <= 0:
            ended = "idle" if idle_first else "timeout"
            break
        try:
            chunk = await asyncio.wait_for(uart_iface.read_all(flush=False), wait)
        except asyncio.TimeoutError:
            ended = "idle" if idle_first else "timeout"
            break
        got += bytes(chunk)
        last = time.monotonic()
        if until and got.endswith(until):
            ended = "until"
            break
        if len(got) >= p["max_bytes"]:
            ended = "max_bytes"
            break
    got = got[: p["max_bytes"]]
    return {"sent": len(p["send"]), "received": len(got), "hex": bytes(got).hex(), "ended_by": ended,
            "elapsed_s": round(time.monotonic() - t0, 4), "baud_measured": await uart_iface.get_baud()}


async def _gpio_pattern(ns, p):
    # [@ANCHOR: glasgow_ops_gpio_pattern]
    gpio_iface = ns["gpio_iface"]
    n = gpio_iface.count
    stamps = []
    try:
        for i in range(n):
            await gpio_iface.output(i, bool((p["first"] >> i) & 1))
        t0 = time.monotonic()
        for _ in range(p["repeat"]):
            for value, hold_ms in p["steps"]:
                await gpio_iface.set_all(value)
                due = time.monotonic() + hold_ms / 1000.0
                await asyncio.sleep(max(0.0, due - time.monotonic()))
                stamps.append(time.monotonic())
                if time.monotonic() - t0 > p["max_s"]:
                    raise RuntimeError("the pattern ran past its time limit of %s s" % p["max_s"])
    finally:
        for i in range(n):
            await gpio_iface.input(i)
    total = stamps[-1] - t0 if stamps else 0.0
    return {"steps_run": len(stamps), "elapsed_s": round(total, 4), "released": True,
            "note": "levels are set from the host: timing is accurate to about a millisecond, not to the FPGA clock"}


async def _swd_idcode(ns, p):
    # [@ANCHOR: glasgow_ops_swd_idcode]
    swd_iface = ns["swd_iface"]
    d = await swd_iface.initialize()
    return {"dpidr": "0x%08x" % d.to_int(), "version": int(d.VERSION), "min": bool(d.MIN), "designer": int(d.DESIGNER),
            "partno": int(d.PARTNO), "revision": int(d.REVISION)}


_OPS = {"i2c_scan": _i2c_scan, "i2c_read": _i2c_read, "i2c_write": _i2c_write, "uart_transact": _uart_transact,
        "gpio_pattern": _gpio_pattern, "swd_idcode": _swd_idcode}


async def main(ns):
    # [@ANCHOR: glasgow_ops_main]
    op, payload = ns["args"].script_args[0], json.loads(ns["args"].script_args[1])
    result = await _OPS[op](ns, payload)
    print("BENCH_GLASGOW_RESULT " + json.dumps(result), flush=True)
