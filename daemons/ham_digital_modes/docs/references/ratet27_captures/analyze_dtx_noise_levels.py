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
with open(__import__('os').path.join(__import__('os').path.dirname(__file__), 'dtx_noise_levels_sweep.tsv')) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts)!=3: continue
        label, idx, hexstr = parts
        by_label[label].append(hexstr)

print(f"{'peak':>10s} {'g0':>15s} {'g2':>15s} {'c7':>10s}")
for label in sorted(by_label.keys(), key=lambda l: float(l.replace('noiselevel_',''))):
    peak = label.replace('noiselevel_','')
    g0s, g2s, c7s = [], [], []
    for hexstr in by_label[label]:
        wire_bits = hex_to_bits(hexstr)
        natural_bits = [0]*144
        for m in range(144):
            natural_bits[natural_position(m)] = wire_bits[m]
        g0,_ = golay_decode(extract_block(natural_bits, 0, 23))
        g2,_ = golay_decode(extract_block(natural_bits, 46, 23))
        c7 = extract_block(natural_bits, 137, 7)
        g0s.append(g0); g2s.append(g2); c7s.append(c7)
    print(f"{peak:>10s} {str(sorted(set(g0s))):>15s} {str(sorted(set(g2s))):>15s} {str(sorted(set(c7s))):>10s}")
