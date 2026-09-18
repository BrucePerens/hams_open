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
by_label = defaultdict(list)
import os
data_path = os.path.join(os.path.dirname(__file__), 'lhat_boundary_sweep.tsv')
with open(data_path) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        by_label[label].append(hexstr)

results = {}
for label in by_label:
    hexstr = by_label[label][-1]
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        natural_bits[natural_position(m)] = wire_bits[m]
    g1 = extract_block(natural_bits, 23, 23)
    g2 = extract_block(natural_bits, 46, 23)
    g1d,_ = golay_decode(g1)
    g2d,_ = golay_decode(g2)
    results[label] = (g1d, g2d)

boundaries = [100.7, 155.4, 202.6, 238.9, 254.0, 271.2, 313.8, 340.5]
for b in boundaries:
    below = b - 3.0
    above = b + 3.0
    below_label = f"boundary_{b}_{below}"
    above_label = f"boundary_{b}_{above}"
    if below_label in results and above_label in results:
        g1b, g2b = results[below_label]
        g1a, g2a = results[above_label]
        print(f"boundary~{b:6.1f}Hz: below(freq={below:.1f}) g1={g1b} g2={g2b}  |  above(freq={above:.1f}) g1={g1a} g2={g2a}  |  g1_changed={g1b!=g1a} g2_changed={g2b!=g2a}")
