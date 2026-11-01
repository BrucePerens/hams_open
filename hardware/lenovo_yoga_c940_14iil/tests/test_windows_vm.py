# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for the Windows VM package: script syntax, the domain XML template, and the libvirt hook's
safety behaviour (refuse to start when /windows cannot be unmounted, never remount while the VM runs),
using stub commands in a temporary PATH.  Nothing here touches a real disk or libvirt.

Run:  python3 -B -m unittest discover -s hardware/lenovo_yoga_c940_14iil/tests
"""
import os
import stat
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET

HERE = os.path.dirname(os.path.abspath(__file__))
VM = os.path.join(os.path.dirname(HERE), "windows-vm")
NS = {"qemu": "http://libvirt.org/schemas/domain/qemu/1.0"}


def put(path, data, mode="w"):
    with open(path, mode) as f:
        f.write(data)


def get(path):
    with open(path) as f:
        return f.read()


def write_exe(path, body):
    with open(path, "w") as f:
        f.write("#!/bin/sh\n" + body + "\n")
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)


class Syntax(unittest.TestCase):
    def test_bash_syntax(self):
        for rel in ("bin/yoga-windows-vm", "lib/common.sh", "hooks/50-yoga-windows-vm"):
            r = subprocess.run(["bash", "-n", os.path.join(VM, rel)], capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, rel + r.stderr)


class DomainXml(unittest.TestCase):
    def setUp(self):
        text = get(os.path.join(VM, "xml/yoga-windows.xml.in"))
        self.root = ET.fromstring(text.replace("@DISK@", "<disk/>").replace("@DATA_DIR@", "/srv/x"))

    def test_required_devices(self):
        r = self.root
        self.assertEqual(r.findtext("name"), "yoga-windows")
        self.assertIsNotNone(r.find("os/smbios[@mode='host']"))
        self.assertIsNotNone(r.find("devices/tpm/backend[@version='2.0']"))
        self.assertEqual(len(r.findall("devices/redirdev[@type='spicevmc']")), 3)
        self.assertIsNotNone(r.find("devices/graphics[@type='spice']/clipboard[@copypaste='yes']"))
        self.assertIsNotNone(r.find("devices/channel/source[@channel='org.spice-space.webdav.0']"))
        self.assertIsNotNone(r.find("devices/interface/backend[@type='passt']"))
        rng = r.find("devices/interface/portForward/range")
        self.assertEqual((rng.get("start"), rng.get("to")), ("2222", "22"))
        self.assertEqual(r.find("cpu").get("mode"), "host-passthrough")
        args = [a.get("value") for a in r.findall("qemu:commandline/qemu:arg", NS)]
        self.assertIn("-acpitable", args)
        self.assertIn("file=@DATA_DIR@/MSDM.aml".replace("@DATA_DIR@", "/srv/x"), args)

    def test_no_hyperv_and_no_autostart(self):
        # Hyper-V enlightenments hang the Windows boot loader here; libvirt autostart must stay off.
        self.assertIsNone(self.root.find("features/hyperv"))
        self.assertIsNone(self.root.find("autostart"))

    def test_no_debian_partition_is_named(self):
        text = get(os.path.join(VM, "xml/yoga-windows.xml.in")) + get(os.path.join(VM, "bin/yoga-windows-vm"))
        for forbidden in ("nvme0n1p4", "nvme0n1p5", "e9982893", "01140132"):
            self.assertNotIn(forbidden, text)

    def test_rendered_modes(self):
        with tempfile.TemporaryDirectory() as d:
            conf = os.path.join(d, "c.conf")
            put(conf, "MODE=overlay\nWIN_PARTUUID=x\nWIN_MOUNT=/w\nDATA_DIR=/srv/t\nSHUTDOWN_WAIT=1\n")
            out = {}
            for mode in ("overlay", "raw"):
                cmd = ("sed -n '/^render_xml/,/^}/p' %s/bin/yoga-windows-vm > %s/rx.sh; . %s/lib/common.sh; . %s/rx.sh; "
                       "SHARE=%s/xml; render_xml %s" % (VM, d, VM, d, VM, mode))
                r = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True, env=dict(os.environ, YWV_CONF=conf))
                self.assertEqual(r.returncode, 0, r.stderr)
                out[mode] = ET.fromstring(r.stdout)
            ov = out["overlay"].find("devices/disk")
            self.assertEqual(ov.find("driver").get("type"), "qcow2")
            self.assertEqual(ov.find("backingStore/source").get("dev"), "/dev/mapper/yoga-windows")
            raw = out["raw"].find("devices/disk")
            self.assertEqual(raw.find("source").get("dev"), "/dev/mapper/yoga-windows")
            self.assertEqual(raw.find("driver").get("cache"), "none")


class HookSafety(unittest.TestCase):
    """Run the real hook with stub umount/dmsetup/losetup/mount/pgrep/lsblk commands."""

    def run_hook(self, op, sub, mounted, vm_running, umount_ok=True):
        with tempfile.TemporaryDirectory() as d:
            bindir = os.path.join(d, "bin")
            os.mkdir(bindir)
            log = os.path.join(d, "log")
            mi = os.path.join(d, "mountinfo")
            put(mi, "1 1 259:3 / /windows rw - ntfs3 /dev/x rw\n" if mounted else "")
            for name, body in {
                "lsblk": "echo ' 259:3'",
                "pgrep": "exit %d" % (0 if vm_running else 1),
                "umount": "echo umount >> %s; exit %d" % (log, 0 if umount_ok else 1),
                "mount": "echo mount >> %s" % log,
                "dmsetup": "case $1 in info) [ -f %s/dm ];; create) echo dm-create >> %s; touch %s/dm; cat >/dev/null;; remove) echo dm-remove >> %s; rm -f %s/dm;; esac" % (d, log, d, log, d),
                "losetup": "case $1 in -d) echo loop-detach >> %s;; *) echo /dev/loop9;; esac" % log,
                "blockdev": "echo 2048",
                "logger": "true",
            }.items():
                write_exe(os.path.join(bindir, name), body)
            os.makedirs(os.path.join(d, "dev"))
            data = os.path.join(d, "data")
            os.mkdir(data)
            for f in ("head.img", "tail.img"):
                put(os.path.join(data, f), b"\0" * 1024, "wb")
            conf = os.path.join(d, "c.conf")
            put(conf, "MODE=overlay\nWIN_PARTUUID=u\nWIN_MOUNT=/w\nDATA_DIR=%s\nSHUTDOWN_WAIT=1\n" % data)
            # common.sh uses /dev/disk/by-partuuid/<uuid>: make the -b test pass through a patched copy.
            lib = os.path.join(d, "lib")
            os.mkdir(lib)
            src = get(os.path.join(VM, "lib/common.sh"))
            src = src.replace("LOCK=/run/yoga-windows-vm.lock", "LOCK=%s/lock" % d).replace("RUNDIR=/run/yoga-windows-vm", "RUNDIR=%s/run" % d)
            src = src.replace("[ -b \"$WIN_DEV\" ]", "true")
            put(os.path.join(lib, "common.sh"), src)
            env = dict(os.environ, PATH=bindir + ":" + os.environ["PATH"], YWV_CONF=conf, YWV_LIB=lib,
                       YWV_MOUNTINFO=mi, YWV_NOLOG="1")
            # After an unmount the stub umount must make win_mounted false: emulate with an empty mountinfo.
            if umount_ok:
                write_exe(os.path.join(bindir, "umount"), "echo umount >> %s; : > %s" % (log, mi))
                write_exe(os.path.join(bindir, "mount"), "echo mount >> %s; echo '1 1 259:3 / /windows rw - ntfs3 x rw' > %s" % (log, mi))
            r = subprocess.run([os.path.join(VM, "hooks/50-yoga-windows-vm"), "yoga-windows", op, sub, "-"],
                               env=env, capture_output=True, text=True, timeout=60)
            entries = get(log).split() if os.path.exists(log) else []
            return r.returncode, entries

    def test_prepare_unmounts_then_maps(self):
        rc, log = self.run_hook("prepare", "begin", mounted=True, vm_running=False)
        self.assertEqual(rc, 0)
        self.assertEqual(log[:2], ["umount", "dm-create"])

    def test_prepare_refused_when_unmount_fails(self):
        rc, log = self.run_hook("prepare", "begin", mounted=True, vm_running=False, umount_ok=False)
        self.assertNotEqual(rc, 0)
        self.assertNotIn("dm-create", log)

    def test_prepare_refused_when_vm_already_running(self):
        rc, log = self.run_hook("prepare", "begin", mounted=False, vm_running=True)
        self.assertNotEqual(rc, 0)
        self.assertNotIn("dm-create", log)

    def test_release_removes_mapping_then_remounts(self):
        rc, log = self.run_hook("release", "end", mounted=False, vm_running=False)
        self.assertEqual(rc, 0)
        self.assertEqual(log[-1], "mount")

    def test_release_never_remounts_while_vm_runs(self):
        rc, log = self.run_hook("release", "end", mounted=False, vm_running=True)
        self.assertNotIn("mount", log)

    def test_other_domains_ignored(self):
        r = subprocess.run([os.path.join(VM, "hooks/50-yoga-windows-vm"), "other", "prepare", "begin", "-"], capture_output=True)
        self.assertEqual(r.returncode, 0)


if __name__ == "__main__":
    unittest.main()
