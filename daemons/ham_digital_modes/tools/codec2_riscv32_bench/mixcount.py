#!/usr/bin/env python3
"""Instruction-class counter for the riscv32imc bench under the Unicorn CPU emulator.

Runs the flat binary of the bench (`llvm-objcopy -O binary`), classifies every executed instruction
(ALU, multiply, divide, load, store, branch taken/not taken, jump) and splits the counts into the
encode phase and the decode phase, so a cycle range can be computed from an assumed per-class latency
table. Usage: mixcount.py bench.bin bench.elf FRAMES
The entry addresses of the harness's out-of-line `do_encode` / `do_decode` wrappers (src/main.rs) are read from the ELF with
`llvm-nm`; the decode phase starts the first time `do_decode` is entered. The bench must be the `small` build (see prep.sh).
"""
import re
import subprocess
import sys
from unicorn import Uc, UC_ARCH_RISCV, UC_MODE_RISCV32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE, UC_PROT_ALL
from unicorn.riscv_const import UC_RISCV_REG_PC

BASE = 0x80000000


def symbol_address(elf, name):
    out = subprocess.run(["llvm-nm", elf], capture_output=True, text=True, check=True).stdout
    hits = [int(m.group(1), 16) for m in re.finditer(r"^([0-9a-f]+) [tT] \S*" + name + r"\S*$", out, re.M)]
    if len(hits) != 1:
        sys.exit(f"expected exactly one symbol matching {name!r} in {elf}, found {len(hits)}")
    return hits[0]


binf, elf, frames = sys.argv[1], sys.argv[2], int(sys.argv[3])
enc_addr, dec_addr = symbol_address(elf, "do_encode"), symbol_address(elf, "do_decode")
code = open(binf, "rb").read()

uc = Uc(UC_ARCH_RISCV, UC_MODE_RISCV32)
uc.mem_map(BASE, 32 * 1024 * 1024, UC_PROT_ALL)
uc.mem_map(0x10000000, 0x1000, UC_PROT_ALL)   # UART (writes ignored)
uc.mem_map(0x100000, 0x1000, UC_PROT_ALL)     # test finisher
uc.mem_write(BASE, code)

CLASSES = ["alu", "mul", "div", "load", "store", "branch", "jump", "csr"]
cache = {}

def classify(insn16, insn32):
    if insn16 & 3 != 3:
        q, f3 = insn16 & 3, (insn16 >> 13) & 7
        if q == 0:
            return "load" if f3 == 2 else "store" if f3 == 6 else "alu"
        if q == 1:
            if f3 in (1, 5): return "jump"
            if f3 in (6, 7): return "branch"
            return "alu"
        if f3 == 2: return "load"
        if f3 == 6: return "store"
        if f3 == 4:
            rs2 = (insn16 >> 2) & 31
            bit12 = (insn16 >> 12) & 1
            if rs2 == 0 and (insn16 >> 7) & 31: return "jump"      # c.jr / c.jalr
            return "alu"
        return "alu"
    op = insn32 & 0x7F
    if op == 0x03: return "load"
    if op == 0x23: return "store"
    if op == 0x63: return "branch"
    if op in (0x6F, 0x67): return "jump"
    if op == 0x73: return "csr"
    if op == 0x33 and (insn32 >> 25) == 1:
        return "mul" if ((insn32 >> 12) & 7) < 4 else "div"
    return "alu"

counts = {"enc": {}, "dec": {}}
phase = "enc"
pending_branch = None   # (class, addr, size)
state = {"phase": "enc", "prev": None, "n": 0}
def bump(ph, k):
    d = counts[ph]; d[k] = d.get(k, 0) + 1

def hook(uc, addr, size, _):
    if addr == dec_addr and state["phase"] == "enc":
        state["phase"] = "dec"
    prev = state["prev"]
    if prev is not None:
        pcls, paddr, psize = prev
        if pcls == "branch":
            bump(state["pphase"], "branch_taken" if addr != paddr + psize else "branch_not_taken")
        # jumps are always taken
    ent = cache.get(addr)
    if ent is None:
        raw = bytes(uc.mem_read(addr, 4))
        i16 = int.from_bytes(raw[:2], "little")
        i32 = int.from_bytes(raw, "little")
        ent = classify(i16, i32)
        cache[addr] = ent
    if ent != "branch":
        bump(state["phase"], ent)
    state["prev"] = (ent, addr, size)
    state["pphase"] = state["phase"]

def memw(uc, access, address, size, value, _):
    if address == 0x100000:
        uc.emu_stop()

uc.hook_add(UC_HOOK_CODE, hook)
uc.hook_add(UC_HOOK_MEM_WRITE, memw)
try:
    uc.emu_start(BASE, BASE + 0x1000000, timeout=0, count=0)
except Exception as e:
    print("emulation ended:", e)
for ph in ("enc", "dec"):
    c = counts[ph]
    tot = sum(v for k, v in c.items() if k not in ("branch_taken", "branch_not_taken")) + c.get("branch_taken", 0) + c.get("branch_not_taken", 0)
    print(ph, "total instr", tot, "per frame", tot // frames)
    for k in sorted(c): print("   %-18s %10d  %6.2f%%" % (k, c[k], 100.0 * c[k] / tot))
import json
json.dump(counts, open(sys.argv[1] + ".mix.json", "w"))
