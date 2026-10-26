#!/usr/bin/python3 -I
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Power-cycle stress test for an SDRplay RSP on a port whose power you can switch.

Each iteration: power the port off, wait for the receiver to leave the bus, wait, power it on,
wait for it to enumerate, check that it came back in its cold state (bcdDevice 2.00) with the
expected USB ID, then run the `rsp_coldcheck` example (one pass/fail JSON line). Every
iteration is logged as one JSON line. The run stops at the first failure and leaves the device
powered, with a snapshot (lsusb -v for the vendor, the kernel log tail), so it can be inspected;
use --keep-going to continue instead.

Power backends (--backend):
  none       no power control: only repeats the check (--hammer opens per iteration).
  uhubctl    `uhubctl -l LOCATION -p PORT -a off|on` (needs uhubctl and a hub that really
             switches port power; check with `uhubctl` and by watching the device disappear).
  sysfs      writes the port's `disable` file (does NOT cut VBUS on many machines, including
             the one this was written on: the receiver kept its state; use only to test).
  cmd        --cmd-off and --cmd-on are commands (relay boards, smart plugs, ...), split like a shell
             line and run without a shell: no pipes or redirections; wrap those in a script.
  manual     asks you to unplug and replug, and waits for the device to appear.

Example (a Raspberry Pi whose USB port power is switchable):
  power_cycle_stress.py --backend uhubctl --location 2 --port 1 --model rsp2 \\
      --coldcheck ./target/release/examples/rsp_coldcheck --iterations 500 --log run.jsonl

Nothing may be connected to the receiver's antenna port. The test only sends requests that
rsp_open documents; it never issues a USB reset.
"""
import argparse
import json
import os
import shlex
import subprocess
import sys
import time

VID = "1df7"
PIDS = {"rsp2": "3010", "rsp1a": "3000", "rsp1": "2500"}


def run(cmd, timeout=60):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return 124, "", "timeout"


def sdrplay_devices():
    """(pid, bcdDevice, sysfs path) of every SDRplay device, from sysfs (nothing is opened)."""
    out = []
    base = "/sys/bus/usb/devices"
    for d in sorted(os.listdir(base)):
        p = os.path.join(base, d)
        try:
            if open(os.path.join(p, "idVendor")).read().strip() != VID:
                continue
            pid = open(os.path.join(p, "idProduct")).read().strip()
            bcd = open(os.path.join(p, "bcdDevice")).read().strip()
            out.append((pid, bcd, p))
        except OSError:
            continue  # not a readable USB device entry
    return out


def wait_for(pred, timeout, step=0.2):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(step)
    return None


class Power:
    def __init__(self, a):
        self.a = a

    def _uhubctl(self, action):
        cmd = ["uhubctl", "-l", self.a.location, "-p", self.a.port, "-a", action]
        if self.a.uhubctl_extra:
            cmd += self.a.uhubctl_extra.split()
        rc, out, err = run(cmd, timeout=30)
        return rc == 0, (out + err)[-300:]

    def off(self):
        b = self.a.backend
        if b == "none":
            return True, ""
        if b == "uhubctl":
            return self._uhubctl("off")
        if b == "sysfs":
            return self._sysfs("1")
        if b == "cmd":
            rc, out, err = run(shlex.split(self.a.cmd_off), timeout=30)
            return rc == 0, (out + err)[-300:]
        if b == "manual":
            input("Unplug the receiver, then press Enter... ")
            return True, ""
        return False, "unknown backend"

    def on(self):
        b = self.a.backend
        if b == "none":
            return True, ""
        if b == "uhubctl":
            return self._uhubctl("on")
        if b == "sysfs":
            return self._sysfs("0")
        if b == "cmd":
            rc, out, err = run(shlex.split(self.a.cmd_on), timeout=30)
            return rc == 0, (out + err)[-300:]
        if b == "manual":
            input("Plug the receiver back in, then press Enter... ")
            return True, ""
        return False, "unknown backend"

    def _sysfs(self, val):
        rc, out, err = run(["sudo", "-n", "sh", "-c", f"echo {val} > {self.a.sysfs_disable}"], timeout=10)
        return rc == 0, (out + err)[-300:]


def snapshot():
    snap = {}
    snap["devices"] = [(p, b) for p, b, _ in sdrplay_devices()]
    rc, out, err = run(["lsusb", "-v", "-d", VID + ":"], timeout=20)
    snap["lsusb_v"] = out[-1500:]
    rc, out, err = run(["sh", "-c", "dmesg 2>/dev/null | tail -25 || true"], timeout=10)
    snap["dmesg_tail"] = out[-1500:]
    return snap


def one_iteration(a, power, n):
    rec = {"n": n, "t": time.strftime("%Y-%m-%dT%H:%M:%S"), "ok": False}
    want_pid = PIDS[a.model] if a.model else None
    if a.backend != "none":
        ok, msg = power.off()
        rec["power_off"] = ok
        if not ok:
            rec["error"] = "power off failed: " + msg
            return rec
        gone = wait_for(lambda: not sdrplay_devices(), a.gone_timeout)
        rec["left_bus"] = bool(gone)
        if not gone and not a.allow_stay_on_bus:
            rec["error"] = "receiver did not leave the bus after power off (port power not really switched?)"
            return rec
        time.sleep(a.off_seconds)
        ok, msg = power.on()
        rec["power_on"] = ok
        if not ok:
            rec["error"] = "power on failed: " + msg
            return rec
        dev = wait_for(lambda: (sdrplay_devices() or None), a.appear_timeout)
        if not dev:
            rec["error"] = f"receiver did not enumerate within {a.appear_timeout:.0f} s after power on"
            return rec
        pid, bcd, _ = dev[0]
        rec["pid"], rec["bcd"] = pid, bcd
        if want_pid and pid != want_pid:
            rec["error"] = f"wrong USB ID after power cycle: {VID}:{pid}, expected {VID}:{want_pid}"
            return rec
        if bcd != "0200":
            rec["error"] = f"not in the cold state after power cycle: bcdDevice {bcd}"
            return rec
        time.sleep(a.settle_seconds)
    runs = []
    for k in range(a.hammer):
        cmd = [a.coldcheck] + ([a.model] if a.model else []) + (["--running"] if a.running else [])
        rc, out, err = run(cmd, timeout=a.check_timeout)
        line = out.strip().splitlines()[-1] if out.strip() else ""
        try:
            res = json.loads(line)
        except ValueError:
            res = {"ok": False, "error": f"no JSON from coldcheck (rc {rc}): {(out + err)[-200:]}"}
        runs.append(res)
        if not res.get("ok"):
            break
    rec["checks"] = runs
    rec["ok"] = bool(runs) and all(r.get("ok") for r in runs)
    if not rec["ok"]:
        bad = [r for r in runs if not r.get("ok")]
        rec["error"] = "coldcheck failed: " + str(bad[0].get("error", bad[0]))[:300]
    return rec


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--backend", choices=["none", "uhubctl", "sysfs", "cmd", "manual"], default="none")
    ap.add_argument("--location", help="uhubctl hub location (-l)")
    ap.add_argument("--port", help="uhubctl port (-p)")
    ap.add_argument("--uhubctl-extra", default="", help="extra uhubctl arguments, e.g. '-N'")
    ap.add_argument("--sysfs-disable", help="path of the port's `disable` file (sysfs backend)")
    ap.add_argument("--cmd-off")
    ap.add_argument("--cmd-on")
    ap.add_argument("--model", choices=sorted(PIDS), help="expected receiver model")
    ap.add_argument("--coldcheck", required=True, help="path of the rsp_coldcheck example binary")
    ap.add_argument("--running", action="store_true", help="check a firmware-loaded receiver instead of a cold one")
    ap.add_argument("--iterations", type=int, default=100)
    ap.add_argument("--hammer", type=int, default=1, help="coldcheck runs (open/close cycles) per power cycle")
    ap.add_argument("--off-seconds", type=float, default=5.0)
    ap.add_argument("--settle-seconds", type=float, default=1.0)
    ap.add_argument("--gone-timeout", type=float, default=10.0)
    ap.add_argument("--appear-timeout", type=float, default=20.0)
    ap.add_argument("--check-timeout", type=float, default=60.0)
    ap.add_argument("--allow-stay-on-bus", action="store_true",
                    help="continue even if the device stays on the bus after power off")
    ap.add_argument("--keep-going", action="store_true", help="do not stop at the first failure")
    ap.add_argument("--log", default="power_cycle_stress.jsonl")
    a = ap.parse_args()
    if a.backend == "uhubctl" and not (a.location and a.port):
        ap.error("--backend uhubctl needs --location and --port")
    if a.backend == "sysfs" and not a.sysfs_disable:
        ap.error("--backend sysfs needs --sysfs-disable")
    if a.backend == "cmd" and not (a.cmd_off and a.cmd_on):
        ap.error("--backend cmd needs --cmd-off and --cmd-on")
    power = Power(a)
    passed = failed = 0
    with open(a.log, "a") as log:
        for n in range(1, a.iterations + 1):
            rec = one_iteration(a, power, n)
            if rec["ok"]:
                passed += 1
            else:
                failed += 1
                rec["snapshot"] = snapshot()
            log.write(json.dumps(rec) + "\n")
            log.flush()
            print(f"[{n}/{a.iterations}] {'ok' if rec['ok'] else 'FAIL: ' + rec.get('error', '?')}", flush=True)
            if not rec["ok"] and not a.keep_going:
                print("stopped at the first failure; the receiver is left powered for inspection", file=sys.stderr)
                break
    print(f"passed {passed}, failed {failed}, log {a.log}")
    sys.exit(0 if failed == 0 else 1)


if __name__ == "__main__":
    main()
