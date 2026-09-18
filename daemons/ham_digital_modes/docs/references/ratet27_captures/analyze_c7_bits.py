def natural_position(m):
    return 12*(m % 12) + (m // 12)
def hex_to_bits(hexstr):
    data = bytes.fromhex(hexstr)
    bits = []
    for byte in data:
        for i in range(8):
            bits.append((byte >> (7-i)) & 1)
    return bits

frames = []
import os
data_path = os.path.join(os.path.dirname(__file__), 'all_stimuli_2687frames.tsv')
with open(data_path) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        wire_bits = hex_to_bits(hexstr)
        natural_bits = [0]*144
        for m in range(144):
            natural_bits[natural_position(m)] = wire_bits[m]
        c7 = natural_bits[137:144]
        frames.append((label, c7))

print(f"Total frames: {len(frames)}")
for bit_idx in range(7):
    vals = [f[1][bit_idx] for f in frames]
    ones = sum(vals)
    print(f"c7 bit {bit_idx} (natural offset {137+bit_idx}): {ones}/{len(vals)} ones ({100*ones/len(vals):.1f}%)")

# correlation with sine/sawtooth frequency for known-frequency labels
import re
freq_frames = []
for label, c7 in frames:
    m = re.match(r'(sine|sawtooth|extreme_sine)_(\d+\.?\d*)$', label)
    if m:
        freq = float(m.group(2))
        freq_frames.append((freq, c7))

def spearman(xs, ys):
    n = len(xs)
    def rank(vals):
        si = sorted(range(len(vals)), key=lambda i: vals[i])
        r = [0]*len(vals)
        for rk,i in enumerate(si): r[i]=rk
        return r
    rx, ry = rank(xs), rank(ys)
    d2 = sum((a-b)**2 for a,b in zip(rx,ry))
    return 1 - (6*d2)/(n*(n**2-1)) if n>1 else 0

print(f"\nFreq-labeled frames: {len(freq_frames)}")
freqs = [f[0] for f in freq_frames]
for bit_idx in range(7):
    vals = [f[1][bit_idx] for f in freq_frames]
    print(f"c7 bit {bit_idx}: Spearman(freq, bit) = {spearman(freqs, vals):.3f}")
