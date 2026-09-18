import math
def natural_position(m):
    return 12*(m % 12) + (m // 12)
def hex_to_bits(hexstr):
    data = bytes.fromhex(hexstr)
    bits = []
    for byte in data:
        for i in range(8):
            bits.append((byte >> (7-i)) & 1)
    return bits
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

from collections import defaultdict
freqs_hex = defaultdict(list)
with open('/home/bruce/workspace/hams_open/daemons/ham_digital_modes/docs/references/ratet27_captures/rms_normalized_pitch_sweep_57to444hz.tsv') as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        freq = float(label.replace('rmsnorm_',''))
        freqs_hex[freq].append(hexstr)

results = []
for freq in sorted(freqs_hex.keys()):
    # use ALL captured frames for this freq, not just last, to see internal consistency
    u6_vals = []
    for hexstr in freqs_hex[freq]:
        wire_bits = hex_to_bits(hexstr)
        natural_bits = [0]*144
        for m in range(144):
            natural_bits[natural_position(m)] = wire_bits[m]
        u6,_ = hamming_decode_chip(extract_block(natural_bits, 122, 15))
        u6_vals.append(u6)
    results.append((freq, u6_vals))

print(f"{'freq':>6s}  u6 (all captured frames)")
for freq, vals in results:
    print(f"{freq:6.0f}  {vals}  distinct={len(set(vals))}")
