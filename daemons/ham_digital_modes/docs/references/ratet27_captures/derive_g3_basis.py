import json
import glob
import os

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
seen_hex = set()
here = os.path.dirname(__file__)
for tsv_path in sorted(glob.glob(os.path.join(here, '*.tsv'))):
  with open(tsv_path) as f:
    for line in f:
        parts = line.strip().split('\t')
        if len(parts) != 3: continue
        label, idx, hexstr = parts
        if len(hexstr) != 36 or hexstr in seen_hex:
            continue
        seen_hex.add(hexstr)
        wire_bits = hex_to_bits(hexstr)
        natural_bits = [0]*144
        for m in range(144):
            natural_bits[natural_position(m)] = wire_bits[m]
        frames.append(natural_bits)

print(f"Total distinct frames: {len(frames)}")

def gf2_rref(rows, n_cols):
    rows = [r[:] for r in rows]
    pivots = []
    rank = 0
    for col in range(n_cols):
        pivot = None
        for r in range(rank, len(rows)):
            if rows[r][col] == 1:
                pivot = r
                break
        if pivot is None:
            continue
        rows[rank], rows[pivot] = rows[pivot], rows[rank]
        for r in range(len(rows)):
            if r != rank and rows[r][col] == 1:
                rows[r] = [a^b for a,b in zip(rows[r], rows[rank])]
        pivots.append(col)
        rank += 1
        if rank == len(rows):
            break
    return rows[:rank], pivots, rank

g3_rows = list({tuple(f[69:69+23]) for f in frames})
g3_rows = [list(r) for r in g3_rows]
rref, pivots, rank = gf2_rref(g3_rows, 23)
print(f"g3: {len(g3_rows)} distinct rows, rank={rank}, pivot_cols={pivots}")
print("RREF basis:")
for row in rref:
    print(''.join(str(b) for b in row))

with open(os.path.join(here, 'g3_basis.json'), 'w') as f:
    json.dump({'rank': rank, 'pivots': pivots, 'basis': rref}, f)

# Validate: every captured g3 row must be a linear combination of the 8 basis vectors (i.e. in the
# span) -- confirms the basis is COMPLETE (not just a partial approximation missing real codewords).
def in_span(row, basis):
    # Gaussian elimination membership test
    basis = [b[:] for b in basis]
    row = row[:]
    for b in basis:
        # find pivot of b
        pivot = next((i for i,v in enumerate(b) if v==1), None)
        if pivot is None: continue
        if row[pivot] == 1:
            row = [a^c for a,c in zip(row,b)]
    return all(v==0 for v in row)

all_in_span = all(in_span(list(r), rref) for r in g3_rows)
print(f"\nAll {len(g3_rows)} captured g3 rows in span of derived basis: {all_in_span}")

# Print basis as Rust array-of-u32 (each row as its 23-bit integer, MSB-first matching offset 0..22)
print("\nRust basis (23-bit MSB-first per row):")
for row in rref:
    val = 0
    for b in row:
        val = (val<<1)|b
    print(f"0b{val:023b}, // 0x{val:06x}")
