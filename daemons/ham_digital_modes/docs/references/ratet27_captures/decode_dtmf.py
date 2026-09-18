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
by_digit = defaultdict(list)
with open(__import__('os').path.join(__import__('os').path.dirname(__file__), 'real_dtmf_sweep.tsv')) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts)!=3: continue
        label, idx, hexstr = parts
        by_digit[label].append(hexstr)

print(f"{'digit':>10s} {'g0':>6s} {'g1':>6s} {'g2':>6s} {'g3':>6s} {'g3_dist':>8s}")
for digit in sorted(by_digit.keys()):
    hexstr = by_digit[digit][-1]
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        natural_bits[natural_position(m)] = wire_bits[m]
    vals = []
    dists = []
    for start in [0,23,46,69]:
        block = extract_block(natural_bits, start, 23)
        d, dist = golay_decode(block)
        vals.append(d)
        dists.append(dist)
    print(f"{digit:>10s} {vals[0]:6d} {vals[1]:6d} {vals[2]:6d} {vals[3]:6d} {dists[3]:8d}")

# check g3 distinct count across all DTMF frames
g3_vals = set()
for digit in by_digit:
    for hexstr in by_digit[digit]:
        wire_bits = hex_to_bits(hexstr)
        natural_bits = [0]*144
        for m in range(144):
            natural_bits[natural_position(m)] = wire_bits[m]
        block = extract_block(natural_bits, 69, 23)
        d,_ = golay_decode(block)
        g3_vals.add(d)
print(f"\nDistinct g3 values across all DTMF frames: {len(g3_vals)}: {sorted(g3_vals)}")

HAMMING_PARITY_CHIP = [0b1001,0b1101,0b1111,0b1110,0b0111,0b1010,0b0101,0b1011,0b1100,0b0110,0b0011]
def hamming_encode_chip(data):
    data &= 0x7FF
    parity = 0
    for i in range(11):
        if (data >> (10-i)) & 1:
            parity ^= HAMMING_PARITY_CHIP[i]
    return (data<<4)|parity
def hamming_decode_chip(received):
    received &= 0x7FFF
    best_data, best_dist = 0, 99
    for data in range(2048):
        cw = hamming_encode_chip(data)
        dist = bin(cw^received).count('1')
        if dist < best_dist:
            best_dist = dist
            best_data = data
    return best_data, best_dist

print(f"\n{'digit':>10s} {'u4':>6s} {'u5':>6s} {'u6':>6s} {'c7':>6s}")
for digit in sorted(by_digit.keys()):
    hexstr = by_digit[digit][-1]
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        natural_bits[natural_position(m)] = wire_bits[m]
    u4 = extract_block(natural_bits, 92, 15)
    u5 = extract_block(natural_bits, 107, 15)
    u6 = extract_block(natural_bits, 122, 15)
    c7 = extract_block(natural_bits, 137, 7)
    u4d,_ = hamming_decode_chip(u4)
    u5d,_ = hamming_decode_chip(u5)
    u6d,_ = hamming_decode_chip(u6)
    print(f"{digit:>10s} {u4d:6d} {u5d:6d} {u6d:6d} {c7:6d}")
