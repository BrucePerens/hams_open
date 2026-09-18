def natural_position(m):
    return 12*(m % 12) + (m // 12)

def hex_to_bits(hexstr):
    data = bytes.fromhex(hexstr)
    bits = []
    for byte in data:
        for i in range(8):
            bits.append((byte >> (7-i)) & 1)
    return bits

GOLAY_PARITY = [
    0b110_0011_1010, 0b011_0001_1101, 0b111_1011_0100, 0b011_1101_1010,
    0b001_1110_1101, 0b110_1100_1100, 0b011_0110_0110, 0b001_1011_0011,
    0b110_1110_0011, 0b101_0100_1011, 0b100_1001_1111, 0b100_0111_0101,
]
def golay_encode(data):
    data &= 0xFFF
    parity = 0
    for i in range(12):
        if (data >> (11-i)) & 1:
            parity ^= GOLAY_PARITY[i]
    return (data << 11) | parity

def golay_decode(received):
    received &= 0x7FFFFF
    best_data, best_dist = 0, 99
    for data in range(4096):
        cw = golay_encode(data)
        dist = bin(cw ^ received).count('1')
        if dist < best_dist:
            best_dist = dist
            best_data = data
    return best_data, best_dist

def extract_block(natural_bits, start, length):
    bits = natural_bits[start:start+length]
    val = 0
    for b in bits:
        val = (val<<1)|b
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

results = []
for freq in sorted(freqs_hex.keys()):
    g0_vals = []
    for hexstr in freqs_hex[freq][-4:]:  # last 4 (most converged) of the 8 captured
        wire_bits = hex_to_bits(hexstr)
        natural_bits = [0]*144
        for m in range(144):
            n = natural_position(m)
            natural_bits[n] = wire_bits[m]
        g0 = extract_block(natural_bits, 0, 23)
        g0_data, dist = golay_decode(g0)
        g0_vals.append(g0_data)
    results.append((freq, g0_vals))

print(f"{'freq':>8s}  g0_data values (last 4 captured frames)")
for freq, vals in results:
    print(f"{freq:8.0f}  {vals}")

def spearman(xs, ys):
    n = len(xs)
    def rank(vals):
        sorted_idx = sorted(range(len(vals)), key=lambda i: vals[i])
        ranks = [0]*len(vals)
        for r, i in enumerate(sorted_idx):
            ranks[i] = r
        return ranks
    rx, ry = rank(xs), rank(ys)
    d2 = sum((a-b)**2 for a,b in zip(rx,ry))
    return 1 - (6*d2)/(n*(n**2-1)) if n>1 else 0

freqs = [r[0] for r in results]
g0_last = [r[1][-1] for r in results]
g0_mean = [sum(r[1])/len(r[1]) for r in results]
print(f"\nSpearman(freq, g0_data_last) = {spearman(freqs, g0_last):.3f}")
print(f"Spearman(freq, g0_data_mean) = {spearman(freqs, g0_mean):.3f}")

print("\n--- checking all 8 captured frames per frequency for internal consistency ---")
for freq in sorted(freqs_hex.keys()):
    all_vals = []
    for hexstr in freqs_hex[freq]:
        wire_bits = hex_to_bits(hexstr)
        natural_bits = [0]*144
        for m in range(144):
            n = natural_position(m)
            natural_bits[n] = wire_bits[m]
        g0 = extract_block(natural_bits, 0, 23)
        g0_data, dist = golay_decode(g0)
        all_vals.append(g0_data)
    distinct = set(all_vals)
    print(f"freq={freq:6.0f}  all 8 frames: {all_vals}  distinct={len(distinct)}")
