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
with open(__import__('os').path.join(__import__('os').path.dirname(__file__), 'lhat_controlled_test.tsv')) as f:
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

groups = [
    ("155.4Hz boundary (L_hat 24->23)", 150.0, 155.0, 160.0),
    ("202.6Hz boundary (L_hat 18->17)", 195.0, 200.0, 205.0),
    ("271.2Hz boundary (L_hat 13->12)", 266.0, 271.0, 276.0),
]
for name, f_low, f_mid, f_high in groups:
    g1_low, g2_low = results[f"ctrl_{int(f_low)}"]
    g1_mid, g2_mid = results[f"ctrl_{int(f_mid)}"]
    g1_high, g2_high = results[f"ctrl_{int(f_high)}"]
    print(f"\n{name}:")
    print(f"  {f_low}Hz: g1={g1_low} g2={g2_low}")
    print(f"  {f_mid}Hz: g1={g1_mid} g2={g2_mid}")
    print(f"  {f_high}Hz: g1={g1_high} g2={g2_high}")
    print(f"  SAME-L_hat pair ({f_low},{f_mid}): g1 same={g1_low==g1_mid}  g2 same={g2_low==g2_mid}")
    print(f"  CROSSING pair ({f_mid},{f_high}): g1 same={g1_mid==g1_high}  g2 same={g2_mid==g2_high}")
