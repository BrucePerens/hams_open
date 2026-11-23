# SPDX-License-Identifier: GPL-2.0-or-later
import importlib.util
import os
import pathlib
import struct
import tempfile
import unittest

_here = pathlib.Path(__file__).resolve().parents[1]


def _load(name):
    spec = importlib.util.spec_from_file_location(name, _here / (name + ".py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


kconfig = _load("check_kconfig")
modabi = _load("compare_modabi")


def make_ko(path, vermagic, text=b"\x90\x90"):
    """A minimal little-endian ELF64 with .text, .modinfo and .shstrtab sections."""
    names = b"\0.text\0.modinfo\0.shstrtab\0"
    info = b"vermagic=" + vermagic.encode() + b"\0"
    body = text + info + names
    base = 64
    off_text, off_info, off_names = base, base + len(text), base + len(text) + len(info)
    shoff = base + len(body)

    def sh(name, off, size):
        return struct.pack("<IIQQQQIIQQ", name, 1, 0, 0, off, size, 0, 0, 1, 0)

    hdr = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9) + struct.pack("<HHIQQQIHHHHHH", 1, 62, 1, 0, 0, shoff, 0, 64, 0, 0, 64, 4, 3)
    shdrs = bytes(64) + sh(1, off_text, len(text)) + sh(7, off_info, len(info)) + sh(15, off_names, len(names))
    with open(path, "wb") as fh:
        fh.write(hdr + body + shdrs)


class CheckKconfigTest(unittest.TestCase):
    def _run(self, pub, tree, linux):
        with tempfile.TemporaryDirectory() as d:
            paths = []
            for name, text in (("p", pub), ("t", tree), ("l", linux)):
                p = os.path.join(d, name)
                with open(p, "w", encoding="utf-8") as fh:
                    fh.write(text)
                paths.append(p)
            return kconfig.main(["x"] + paths)

    # Tests [@ANCHOR: aredn_jool:check_kconfig_main]
    def test_net_ns_mismatch_refused(self):
        pub = "# CONFIG_KERNEL_NAMESPACES is not set\n"
        self.assertEqual(self._run(pub, pub, "# CONFIG_NET_NS is not set\n"), 0)
        self.assertEqual(self._run(pub, pub, "CONFIG_NET_NS=y\n"), 1)
        self.assertEqual(self._run(pub, "CONFIG_KERNEL_NAMESPACES=y\n", "# CONFIG_NET_NS is not set\n"), 1)

    # Tests [@ANCHOR: aredn_jool:check_kconfig_parse]
    def test_parse(self):
        with tempfile.NamedTemporaryFile("w", suffix=".config", delete=False) as fh:
            fh.write("CONFIG_A=y\n# CONFIG_B is not set\nCONFIG_C=\"x\"\n")
        try:
            self.assertEqual(kconfig.parse(fh.name), {"CONFIG_A": "y", "CONFIG_B": "n", "CONFIG_C": '"x"'})
        finally:
            os.unlink(fh.name)


class CompareModAbiTest(unittest.TestCase):
    def _pair(self, a, b):
        d = tempfile.mkdtemp()
        for sub, (vm, text) in (("rel", a), ("bld", b)):
            os.makedirs(os.path.join(d, sub))
            make_ko(os.path.join(d, sub, "m.ko"), vm, text)
        return d

    # Tests [@ANCHOR: aredn_jool:compare_modabi_main]
    def test_identical_and_different(self):
        same = self._pair(("6.12.94 SMP", b"\x90\x91"), ("6.12.94 SMP", b"\x90\x91"))
        self.assertEqual(modabi.main(["x", same + "/rel", same + "/bld"]), 0)
        self.assertEqual(modabi.main(["x", same + "/rel", same + "/bld", "--min-modules", "2"]), 1)
        diff = self._pair(("6.12.94 SMP", b"\x90\x91"), ("6.12.94 SMP", b"\x90\x92"))
        self.assertEqual(modabi.main(["x", diff + "/rel", diff + "/bld"]), 1)
        vm = self._pair(("6.12.94 SMP", b"\x90"), ("6.12.94 SMP PREEMPT", b"\x90"))
        self.assertEqual(modabi.main(["x", vm + "/rel", vm + "/bld"]), 1)

    # Tests [@ANCHOR: aredn_jool:compare_modabi_abi]
    # Tests [@ANCHOR: aredn_jool:compare_modabi_sections]
    def test_abi_reads_vermagic(self):
        with tempfile.TemporaryDirectory() as d:
            make_ko(os.path.join(d, "a.ko"), "6.12.94 SMP", b"\x01")
            magic, crcs, code = modabi.abi(os.path.join(d, "a.ko"))
            self.assertEqual(magic, "vermagic=6.12.94 SMP")
            self.assertEqual(crcs, {})
            self.assertEqual(code[".text"], b"\x01")

    # Tests [@ANCHOR: aredn_jool:compare_modabi_find]
    def test_find(self):
        with tempfile.TemporaryDirectory() as d:
            make_ko(os.path.join(d, "a.ko"), "x")
            self.assertEqual(list(modabi.find(d)), ["a.ko"])


if __name__ == "__main__":
    unittest.main()
