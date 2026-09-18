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

def harmonics_count(omega0):
    inner = math.floor(math.pi/omega0 + 0.25)
    return math.floor(0.9254 * inner)
def frequency_bands_count(l_hat):
    if l_hat <= 36:
        return -(-l_hat // 3)  # ceil div
    return 12

from collections import defaultdict
freqs_hex = defaultdict(list)
with open('/home/bruce/workspace/hams_open/daemons/ham_digital_modes/docs/references/ratet27_captures/dense_pitch_sweep_57to444hz.tsv') as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        freq = float(label.replace('dense_',''))
        freqs_hex[freq].append(hexstr)

results = []
for freq in sorted(freqs_hex.keys()):
    hexstr = freqs_hex[freq][-1]
    wire_bits = hex_to_bits(hexstr)
    natural_bits = [0]*144
    for m in range(144):
        natural_bits[natural_position(m)] = wire_bits[m]
    g1 = extract_block(natural_bits, 23, 23)
    g2 = extract_block(natural_bits, 46, 23)
    g1d,_ = golay_decode(g1)
    g2d,_ = golay_decode(g2)
    omega0 = 2*math.pi*freq/8000.0
    l_hat = harmonics_count(omega0)
    k_hat = frequency_bands_count(l_hat)
    results.append((freq, l_hat, k_hat, g1d, g2d))

print(f"{'freq':>6s} {'L_hat':>6s} {'K_hat':>6s} {'g1':>6s} {'g2':>6s}")
for freq, l_hat, k_hat, g1d, g2d in results:
    print(f"{freq:6.0f} {l_hat:6d} {k_hat:6d} {g1d:6d} {g2d:6d}")

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

freqs = [r[0] for r in results]
lhats = [r[1] for r in results]
khats = [r[2] for r in results]
g1s = [r[3] for r in results]
g2s = [r[4] for r in results]
print(f"\nSpearman(L_hat, g1) = {spearman(lhats, g1s):.3f}")
print(f"Spearman(L_hat, g2) = {spearman(lhats, g2s):.3f}")
print(f"Spearman(K_hat, g1) = {spearman(khats, g1s):.3f}")
print(f"Spearman(K_hat, g2) = {spearman(khats, g2s):.3f}")
