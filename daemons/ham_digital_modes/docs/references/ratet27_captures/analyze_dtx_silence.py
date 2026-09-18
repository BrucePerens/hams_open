def natural_position(m):
    return 12*(m % 12) + (m // 12)
def hex_to_bits(hexstr):
    data = bytes.fromhex(hexstr)
    bits = []
    for byte in data:
        for i in range(8):
            bits.append((byte >> (7-i)) & 1)
    return bits

from collections import defaultdict
by_label = defaultdict(list)
with open(__import__('os').path.join(__import__('os').path.dirname(__file__), 'dtx_silence_sweep.tsv')) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts)!=3: continue
        label, idx, hexstr = parts
        by_label[label].append(hexstr)

for label in sorted(by_label.keys()):
    distinct = set(by_label[label])
    print(f"{label}: {len(distinct)} distinct value(s): {sorted(distinct)}")

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
def extract_block(natural_bits, start, length):
    bits = natural_bits[start:start+length]
    val = 0
    for b in bits:
        val = (val<<1)|b
    return val
def decode_full(hexstr):
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        natural_bits[natural_position(m)] = wire_bits[m]
    g = [golay_decode(extract_block(natural_bits, s, 23))[0] for s in [0,23,46,69]]
    u = [hamming_decode_chip(extract_block(natural_bits, s, 15))[0] for s in [92,107,122]]
    c7 = extract_block(natural_bits, 137, 7)
    return g + u + [c7]

print(f"\n{'label':>20s} {'g0':>6s} {'g1':>6s} {'g2':>6s} {'g3':>6s} {'u4':>6s} {'u5':>6s} {'u6':>6s} {'c7':>4s}")
for label in ['dtxoff_silence', 'dtxon_silence']:
    for hexstr in by_label[label][:3]:
        vals = decode_full(hexstr)
        print(f"{label:>20s} " + " ".join(f"{v:6d}" for v in vals))
