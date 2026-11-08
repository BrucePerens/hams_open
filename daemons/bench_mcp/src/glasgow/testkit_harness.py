# SPDX-License-Identifier: AGPL-3.0-or-later
# Test harness for ops.py: evaluates the helper the way `glasgow script` does (top-level await allowed, a context of names) but with
# fake interface objects, so the real helper code is what the tests run. Usage: harness.py HELPER APPLET OP JSON (SIM_CFG in the environment).
import ast
import asyncio
import json
import os
import sys
import types

helper, applet, op, payload = sys.argv[1:5]
cfg = json.loads(os.environ.get("SIM_CFG", "{}"))


class I2CNotAcknowledged(Exception):
    pass


class FakeI2C:
    def __init__(self):
        self.devices = {int(a): bytes.fromhex(d) for a, d in cfg.get("i2c_devices", {}).items()}
        self.log = []

    async def ping(self, a):
        return a in self.devices

    def transaction(self):
        outer = self

        class T:
            async def __aenter__(self):
                return None

            async def __aexit__(self, *a):
                return False
        return T()

    async def write(self, a, data):
        if a not in self.devices:
            raise I2CNotAcknowledged("address %#x not acknowledged" % a)
        self.log.append(("w", a, bytes(data)))

    async def read(self, a, n):
        if a not in self.devices:
            raise I2CNotAcknowledged("address %#x not acknowledged" % a)
        return (self.devices[a] * n)[:n]


class FakeUART:
    def __init__(self):
        self.rx = bytearray(bytes.fromhex(cfg.get("uart_reply", "")))
        self.sent = bytearray()

    async def write(self, data, flush=False):
        self.sent += bytes(data)

    async def read_all(self, flush=True):
        if self.rx:
            d, self.rx = bytes(self.rx), bytearray()
            return memoryview(d)
        await asyncio.sleep(3600)

    async def get_baud(self):
        return 115200.0


class FakeGPIO:
    count = cfg.get("gpio_count", 4)

    def __init__(self):
        self.state = {}

    async def output(self, i, v):
        self.state[i] = ("out", bool(v))

    async def set_all(self, value):
        self.state["all"] = value
        if cfg.get("fail_on_set"):
            raise RuntimeError("simulated failure mid-pattern")

    async def input(self, i):
        self.state[i] = ("in", None)
        with open(os.environ["SIM_RELEASED"], "a") as f:
            f.write("%d\n" % i)


class FakeSWD:
    async def initialize(self):
        class D:
            VERSION, MIN, DESIGNER, PARTNO, REVISION = 2, 0, 0x23b, 0xba, 1

            def to_int(self):
                return 0x2ba01477
        return D()


code = compile(open(helper).read(), helper, "exec", flags=ast.PyCF_ALLOW_TOP_LEVEL_AWAIT)
ns = {"__name__": "<eval>", "asyncio": asyncio, "args": types.SimpleNamespace(script_args=[op, payload]), "device": None,
      "i2c_iface": FakeI2C(), "uart_iface": FakeUART(), "gpio_iface": FakeGPIO(), "swd_iface": FakeSWD()}
result = eval(code, ns)
if asyncio.iscoroutine(result):
    asyncio.run(result)
