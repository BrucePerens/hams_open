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

def gray_decode(g):
    b = g
    shift = 1
    while (g >> shift):
        b ^= (g >> shift)
        shift += 1
    # proper gray decode loop
    mask = g
    val = g
    while mask:
        mask >>= 1
        val ^= mask
    return val

def extract_block(natural_bits, start, length):
    bits = natural_bits[start:start+length]
    val = 0
    for b in bits:
        val = (val<<1)|b
    return val

# Frequency -> hex, from the sibling committed capture file (all_stimuli_2687frames.tsv), which
# includes every sine/sawtooth/noise/speech/dual-tone/chirp frame captured this session.
import os
freqs_hex = {}
data_path = os.path.join(os.path.dirname(__file__), 'all_stimuli_2687frames.tsv')
with open(data_path) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        if label.startswith('sine_') or label.startswith('extreme_sine_'):
            freq = float(label.split('_')[-1])
            freqs_hex.setdefault(freq, []).append(hexstr)

print(f"Frequencies available: {sorted(freqs_hex.keys())}")

results = []
for freq in sorted(freqs_hex.keys()):
    hexstr = freqs_hex[freq][-1]  # last captured (most converged)
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        n = natural_position(m)
        natural_bits[n] = wire_bits[m]
    g0 = extract_block(natural_bits, 0, 23)
    g1 = extract_block(natural_bits, 23, 23)
    g2 = extract_block(natural_bits, 46, 23)
    g3 = extract_block(natural_bits, 69, 23)
    g0_data, _ = golay_decode(g0)
    g1_data, _ = golay_decode(g1)
    g2_data, _ = golay_decode(g2)
    g2_gray = gray_decode(g2_data)
    results.append((freq, g0_data, g1_data, g2_data, g2_gray))

print(f"{'freq':>8s} {'g0_data':>10s} {'g1_data':>10s} {'g2_data':>10s} {'g2_gray':>10s}")
for freq, g0d, g1d, g2d, g2g in results:
    print(f"{freq:8.0f} {g0d:10d} {g1d:10d} {g2d:10d} {g2g:10d}")

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
    return 1 - (6*d2)/(n*(n**2-1))

freqs = [r[0] for r in results]
g0_vals = [r[1] for r in results]
g1_vals = [r[2] for r in results]
g2_vals = [r[3] for r in results]
print(f"\nSpearman(freq, g0_data) = {spearman(freqs, g0_vals):.3f}")
print(f"Spearman(freq, g1_data) = {spearman(freqs, g1_vals):.3f}")
print(f"Spearman(freq, g2_data) = {spearman(freqs, g2_vals):.3f}")

# Only fully-converged frequencies (>=200Hz per established non-convergence pattern)
converged = [r for r in results if r[0] >= 200]
freqs_c = [r[0] for r in converged]
g0_c = [r[1] for r in converged]
print(f"\nConverged-only (>=200Hz) Spearman(freq, g0_data) = {spearman(freqs_c, g0_c):.3f}")
print("converged g0 values:", g0_c)

import math
def pearson(xs, ys):
    n = len(xs)
    mx, my = sum(xs)/n, sum(ys)/n
    cov = sum((x-mx)*(y-my) for x,y in zip(xs,ys))
    vx = sum((x-mx)**2 for x in xs)
    vy = sum((y-my)**2 for y in ys)
    return cov / math.sqrt(vx*vy) if vx and vy else 0.0

# Use only frequencies >=125Hz to avoid the worst non-convergence noise (per established pattern)
usable = [r for r in results if r[0] >= 125]
freqs_u = [r[0] for r in usable]
g0_u = [r[1] for r in usable]
log_freqs = [math.log(f) for f in freqs_u]
inv_freqs = [1.0/f for f in freqs_u]
print(f"\nUsable (>=125Hz): {len(usable)} points")
print(f"Pearson(g0_data, freq)      = {pearson(freqs_u, g0_u):.3f}")
print(f"Pearson(g0_data, log(freq)) = {pearson(log_freqs, g0_u):.3f}")
print(f"Pearson(g0_data, 1/freq)    = {pearson(inv_freqs, g0_u):.3f}")
for f, g in zip(freqs_u, g0_u):
    print(f"  freq={f:6.0f}  g0_data={g:5d}  log(freq)={math.log(f):.3f}  omega0=2pi*f/8000={2*math.pi*f/8000:.4f}")
