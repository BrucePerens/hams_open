import re
import os

data_path = os.path.join(os.path.dirname(__file__), 'ecmode_bit_sweep_output.txt')
sections = {}
cur = None
with open(data_path) as f:
    for line in f:
        m = re.match(r'-- (Baseline|Bit (\d+)): ECMODE_IN=0x([0-9a-f]+) --', line)
        if m:
            cur = m.group(1) if m.group(1) == 'Baseline' else f"bit{m.group(2)}"
            sections[cur] = []
            continue
        if re.match(r'-- Bit (\d+): already tested.*--', line):
            cur = None
            continue
        m = re.search(
            r'g0: (\d+), g1: (\d+), g2: (\d+), g3: (\d+), u4: (\d+), u5: (\d+), u6: (\d+), c7: (\d+)', line
        )
        if m and cur:
            sections[cur].append(tuple(int(x) for x in m.groups()))

names = ['g0', 'g1', 'g2', 'g3', 'u4', 'u5', 'u6', 'c7']
baseline = sections['Baseline']
base_sets = [set(v[i] for v in baseline) for i in range(8)]

print("Baseline value sets (ECMODE_IN=0x0000, 200Hz sawtooth, 8 captured frames):")
for n, s in zip(names, base_sets):
    print(f"  {n}: {sorted(s)}")

print("\nu4 is the cleanest canary: exactly 2 values in baseline, zero jitter beyond that dither.")
print(f"{'bit':>5}  u4 value set")
for i in range(16):
    key = f"bit{i}"
    if key not in sections:
        print(f"{i:5}  (already tested elsewhere: DTX_ENABLE/TD_ENABLE)")
        continue
    u4set = sorted(set(v[4] for v in sections[key]))
    flag = "  <-- DIFFERENT FROM BASELINE" if u4set != sorted(base_sets[4]) else ""
    print(f"{i:5}  {u4set}{flag}")

print("\nFull per-block new-value report (values never seen in baseline):")
for i in range(16):
    key = f"bit{i}"
    if key not in sections:
        continue
    vals = sections[key]
    sets = [set(v[j] for v in vals) for j in range(8)]
    novel = [(n, sorted(sets[j] - base_sets[j])) for j, n in enumerate(names) if sets[j] - base_sets[j]]
    print(f"bit{i}: {novel if novel else 'no new values in any block'}")
