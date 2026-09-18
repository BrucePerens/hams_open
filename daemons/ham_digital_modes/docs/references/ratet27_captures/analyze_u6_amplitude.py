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
by_amp = defaultdict(list)
with open('/home/bruce/workspace/hams_open/daemons/ham_digital_modes/docs/references/ratet27_captures/amplitude_sweep_200hz.tsv') as f:
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
        natural_bits[natural_position(m)] = wire_bits[m]
    u6,_ = hamming_decode_chip(extract_block(natural_bits, 122, 15))
    results.append((amp, u6))

print(f"{'amp':>10s} {'u6':>6s}")
for amp, u6 in results:
    print(f"{amp:10.1f} {u6:6d}")

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
amps = [r[0] for r in results]
u6s = [r[1] for r in results]
print(f"\nSpearman(amp, u6) = {spearman(amps, u6s):.3f}")
