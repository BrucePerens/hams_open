# SPDX-License-Identifier: AGPL-3.0-or-later
"""Record the USB part of /sys and the output of uhubctl, lsusb and
the kernel journal into a fixture directory, for the usb_hub tests.

usage: capture.py OUTDIR   (run as a user that has sudo -n)

Symlinks are kept as links, regular files are copied when small,
and serial numbers and the host name are replaced.
"""
import os
import re
import shutil
import subprocess
import sys

SYS = "/sys"
MAX_FILE = 4096
SKIP_NAMES = {
    "wakeup_last_time_ms",
    "wakeup_max_time_ms",
    "wakeup_total_time_ms",
    "wakeup_active_count",
    "wakeup_abort_count",
    "wakeup_count",
    "wakeup_expire_count",
    "wakeup_prevent_sleep_time_ms",
    "wakeup_active",
    "remove",
    "async",
    "runtime_usage",
    "runtime_suspended_time",
    "runtime_active_time",
    "runtime_active_kids",
    "firmware_node",
}


def copy_tree(src, dst):
    os.makedirs(dst, exist_ok=True)
    for name in sorted(os.listdir(src)):
        path = os.path.join(src, name)
        out = os.path.join(dst, name)
        if name in SKIP_NAMES:
            continue
        if os.path.islink(path):
            copy_link(path, out, name)
        elif os.path.isdir(path):
            copy_tree(path, out)
        else:
            copy_file(path, out, name)


def copy_link(path, out, name):
    target = os.readlink(path)
    if name == "subsystem" or name == "device":
        return
    os.symlink(target, out)


def copy_file(path, out, name):
    try:
        with open(path, "rb") as handle:
            data = handle.read(MAX_FILE + 1)
    except OSError:
        return
    if len(data) > MAX_FILE:
        return
    if name == "serial":
        data = b"SERIAL-REDACTED\n"
    with open(out, "wb") as handle:
        handle.write(data)


def capture_sys(out):
    devices = os.path.join(SYS, "bus/usb/devices")
    seen = set()
    for name in sorted(os.listdir(devices)):
        link = os.path.join(devices, name)
        real = os.path.realpath(link)
        rel = os.path.relpath(real, SYS)
        target = os.path.join(out, "sys", rel)
        parts = rel.split(os.sep)
        bus = [p for p in parts if re.match(r"usb\d+$", p)]
        if bus and bus[0] not in seen and parts[-1] == bus[0]:
            seen.add(bus[0])
            capture_controller(real, rel, out)
            copy_tree(real, target)
    bus_dir = os.path.join(out, "sys/bus/usb/devices")
    os.makedirs(bus_dir, exist_ok=True)
    for name in sorted(os.listdir(devices)):
        link = os.path.join(devices, name)
        dest = os.path.join(bus_dir, name)
        os.symlink(os.readlink(link), dest)


def capture_controller(root_hub, rel, out):
    pci = os.path.dirname(root_hub)
    pci_rel = os.path.relpath(pci, SYS)
    dst = os.path.join(out, "sys", pci_rel)
    os.makedirs(dst, exist_ok=True)
    for name in ("vendor", "device", "class", "uevent", "label"):
        path = os.path.join(pci, name)
        if os.path.isfile(path):
            copy_file(path, os.path.join(dst, name), name)
    drv = os.path.join(pci, "driver")
    if os.path.islink(drv) and not os.path.lexists(
        os.path.join(dst, "driver")
    ):
        os.symlink(os.readlink(drv), os.path.join(dst, "driver"))


def run(command, path, scrub=True):
    done = subprocess.run(
        command, capture_output=True, text=True, check=False
    )
    text = done.stdout + done.stderr
    if scrub:
        text = text.replace(os.uname().nodename, "devbox")
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)


def main():
    out = sys.argv[1]
    if os.path.isdir(out):
        shutil.rmtree(out)
    os.makedirs(out)
    capture_sys(out)
    run(["sudo", "-n", "uhubctl"], out + "/uhubctl.txt")
    run(["sudo", "-n", "uhubctl", "-N"], out + "/uhubctl_nodesc.txt")
    run(["uhubctl"], out + "/uhubctl_noroot.txt")
    run(["lsusb", "-t"], out + "/lsusb_t.txt")
    run(["lsusb", "-t", "-v"], out + "/lsusb_t_v.txt")
    run(["lsusb", "-v", "-s", "3:17"], out + "/lsusb_v_hub.txt")
    run(["cat", "/proc/mounts"], out + "/proc_mounts.txt")
    run(
        ["cat", "/proc/bus/input/devices"],
        out + "/proc_input_devices.txt",
    )
    journal = subprocess.run(
        ["sudo", "-n", "journalctl", "-k", "-o", "short-unix"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout
    keep = trim_journal(
        journal.replace(os.uname().nodename, "devbox")
    )
    with open(out + "/kernel_journal.txt", "w") as handle:
        handle.write("\n".join(keep) + "\n")


def trim_journal(journal):
    """USB lines only: the last 350 apart from the repeated
    'cannot submit urb' lines, of which a few are kept."""
    usb = re.compile(r"usb [0-9]|usb[0-9]|xhci|hub [0-9]")
    lines = [x for x in journal.splitlines() if usb.search(x)]
    urb = [x for x in lines if "cannot submit urb" in x]
    rest = [x for x in lines if "cannot submit urb" not in x]
    bad = re.compile(
        r"error|unable|power cycle|over-current|not respond|reset"
    )
    errors = [x for x in rest if bad.search(x)][-60:]
    merged = set(rest[-350:] + errors + urb[:6] + urb[-2:])
    return sorted(merged, key=lambda x: float(x.split()[0]))


if __name__ == "__main__":
    main()
