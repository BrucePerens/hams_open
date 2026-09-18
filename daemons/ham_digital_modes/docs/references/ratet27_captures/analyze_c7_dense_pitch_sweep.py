def natural_position(m):
    return 12*(m % 12) + (m // 12)
def hex_to_bits(hexstr):
    data = bytes.fromhex(hexstr)
    bits = []
    for byte in data:
        for i in range(8):
            bits.append((byte >> (7-i)) & 1)
    return bits
def gray_decode(g):
    mask = g
    val = g
    while mask:
        mask >>= 1
        val ^= mask
    return val

from collections import defaultdict
freqs_hex = defaultdict(list)
import os
data_path = os.path.join(os.path.dirname(__file__), 'dense_pitch_sweep_57to444hz.tsv')
with open(data_path) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        freq = float(label.replace('dense_',''))
        freqs_hex[freq].append(hexstr)

def spearman(xs, ys):
    n = len(xs)
    def rank(vals):
        si = sorted(range(len(vals)), key=lambda i: vals[i])
        r = [0]*len(vals)
        for rk,i in enumerate(si): r[i]=rk
        return r
    rx, ry = rank(xs), rank(ys)
    d2 = sum((a-b)**2 for a,b in zip(rx,ry))
    return 1 - (6*d2)/(n*(n**2-1))

results = []
for freq in sorted(freqs_hex.keys()):
    hexstr = freqs_hex[freq][-1]
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        natural_bits[natural_position(m)] = wire_bits[m]
    c7bits = natural_bits[137:144]
    val = 0
    for b in c7bits:
        val = (val<<1)|b
    results.append((freq, val, c7bits))

print(f"{'freq':>6s} {'c7(plain)':>10s} {'c7(gray)':>10s}  bits")
for freq, val, bits in results:
    print(f"{freq:6.0f} {val:10d} {gray_decode(val):10d}  {bits}")

freqs = [r[0] for r in results]
plains = [r[1] for r in results]
grays = [gray_decode(r[1]) for r in results]
print(f"\nSpearman(freq, c7_plain) = {spearman(freqs, plains):.3f}")
print(f"Spearman(freq, c7_gray) = {spearman(freqs, grays):.3f}")

# also check per-bit within this clean dataset
for bit_idx in range(7):
    vals = [r[2][bit_idx] for r in results]
    print(f"c7 bit {bit_idx}: Spearman(freq, bit) = {spearman(freqs, vals):.3f}   values: {vals}")
