#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Compare same-named kernel modules of a release (extracted apks) and a build tree: vermagic, CRCs and code/data sections must be equal.
usage: compare_modabi.py RELEASE_DIR BUILD_DIR [--min-modules N]; exit 0 only if >= N modules compared and all identical."""
import os
import struct
import sys


# [@ANCHOR: aredn_jool:compare_modabi_sections]
def sections(data):
    if data[:4] != b"\x7fELF":
        raise ValueError("not ELF")
    is64 = data[4] == 2
    end = "<" if data[5] == 1 else ">"
    if is64:
        shoff, = struct.unpack_from(end + "Q", data, 0x28)
        shentsize, shnum, shstrndx = struct.unpack_from(end + "HHH", data, 0x3A)
    else:
        shoff, = struct.unpack_from(end + "I", data, 0x20)
        shentsize, shnum, shstrndx = struct.unpack_from(end + "HHH", data, 0x2E)
    raw = []
    for i in range(shnum):
        o = shoff + i * shentsize
        if is64:
            name, _t, _f, _a, off, size = struct.unpack_from(end + "IIQQQQ", data, o)
        else:
            name, _t, _f, _a, off, size = struct.unpack_from(end + "IIIIII", data, o)
        raw.append((name, off, size))
    stroff = raw[shstrndx][1]
    out = {}
    for name, off, size in raw:
        s = data[stroff + name:data.index(b"\0", stroff + name)].decode()
        out[s] = data[off:off + size]
    return out, is64, end


# [@ANCHOR: aredn_jool:compare_modabi_abi]
def abi(path):
    with open(path, "rb") as fh:
        data = fh.read()
    secs, is64, end = sections(data)
    entry = 64
    ver = secs.get("__versions", b"")
    crcs = {}
    for i in range(0, len(ver), entry):
        chunk = ver[i:i + entry]
        crc = struct.unpack_from(end + ("Q" if is64 else "I"), chunk, 0)[0]
        name = chunk[8 if is64 else 4:].split(b"\0")[0].decode()
        crcs[name] = crc
    info = secs.get(".modinfo", b"").split(b"\0")
    magic = [x for x in info if x.startswith(b"vermagic=")]
    # CONFIG_MODVERSIONS may be off (AREDN): then only the code and data prove the same kernel config and compiler
    code = {k: v for k, v in secs.items()
            if k.startswith((".text", ".data", ".rodata", ".init", ".exit", ".modinfo", "__ksymtab", ".gnu.linkonce"))
            or k in ("__versions", ".altinstructions")}
    return (magic[0].decode() if magic else ""), crcs, code


# [@ANCHOR: aredn_jool:compare_modabi_find]
def find(root):
    res = {}
    for d, _dirs, files in os.walk(root):
        for f in files:
            if f.endswith(".ko"):
                res.setdefault(f, os.path.join(d, f))
    return res


# [@ANCHOR: aredn_jool:compare_modabi_main]
def main(argv):
    rel, build = argv[1], argv[2]
    need = int(argv[argv.index("--min-modules") + 1]) if "--min-modules" in argv else 1
    relko, buildko = find(rel), find(build)
    ok = bad = 0
    for name in sorted(set(relko) & set(buildko)):
        rm, rc, rcode = abi(relko[name])
        bm, bc, bcode = abi(buildko[name])
        diff = [k for k in set(rc) & set(bc) if rc[k] != bc[k]]
        diff += [k for k in set(rcode) | set(bcode) if k != ".modinfo" and rcode.get(k) != bcode.get(k)][:5]
        if rm != bm or diff:
            bad += 1
            print("MISMATCH %s: vermagic %r vs %r; differs in %s" % (name, rm, bm, diff[:5]))
        else:
            ok += 1
            print("same %s (%d sections, %d CRCs)" % (name, len(rcode), len(rc)))
    print("compared ok=%d mismatched=%d" % (ok, bad))
    return 0 if bad == 0 and ok >= need else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
