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
import os
by_amp = defaultdict(list)
data_path = os.path.join(os.path.dirname(__file__), 'amplitude_sweep_200hz.tsv')
with open(data_path) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        amp = float(label.replace('amp_',''))
        by_amp[amp].append(hexstr)

results = []
for amp in sorted(by_amp.keys()):
    hexstr = by_amp[amp][-1]
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        n = natural_position(m)
        natural_bits[n] = wire_bits[m]
    g0 = extract_block(natural_bits, 0, 23)
    g1 = extract_block(natural_bits, 23, 23)
    g2 = extract_block(natural_bits, 46, 23)
    g0d,_ = golay_decode(g0)
    g1d,_ = golay_decode(g1)
    g2d,_ = golay_decode(g2)
    # check stability across all 8 frames
    all_g1 = []
    for h in by_amp[amp]:
        wb = hex_to_bits(h)
        nb = [0]*144
        for m in range(144):
            nb[natural_position(m)] = wb[m]
        g1v = extract_block(nb, 23, 23)
        g1d2,_ = golay_decode(g1v)
        all_g1.append(g1d2)
    results.append((amp, g0d, g1d, g2d, all_g1))

print(f"{'amp':>10s} {'g0':>6s} {'g1':>6s} {'g2':>6s}  g1_all_8_frames")
for amp, g0d, g1d, g2d, all_g1 in results:
    print(f"{amp:10.1f} {g0d:6d} {g1d:6d} {g2d:6d}  {all_g1}")

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

amps = [r[0] for r in results]
g0s = [r[1] for r in results]
g1s = [r[2] for r in results]
g2s = [r[3] for r in results]
print(f"\nSpearman(amp, g0) = {spearman(amps, g0s):.3f}")
print(f"Spearman(amp, g1) = {spearman(amps, g1s):.3f}")
print(f"Spearman(amp, g2) = {spearman(amps, g2s):.3f}")
