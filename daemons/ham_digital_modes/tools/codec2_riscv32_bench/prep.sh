#!/bin/bash
# Instruction-count harness for the fixed-point Codec2 3200 path on a 32-bit RISC-V core with no
# floating point and no 64-bit multiply (riscv32imc, the ESP32-C3 class), run under QEMU.
#
#   rustup target add riscv32imc-unknown-none-elf     # once; QEMU: qemu-system-riscv32
#   ./prep.sh                                          # copy src/codec2_3200 here with no_std edits
#   cargo build --release                              # (add --features small for a 6-frame build)
#   qemu-system-riscv32 -M virt -m 64M -nographic -bios none -icount shift=0 \
#       -kernel target/riscv32imc-unknown-none-elf/release/codec2_riscv32_bench
#
# The program reads the `minstret` counter (instructions retired) around each stage and prints
# average instructions per 20 ms frame. This counts INSTRUCTIONS, not cycles: cycles per
# instruction on a real core depend on multiplier latency, load/store wait states and flash cache.
# For an instruction-class mix (ALU / multiply / load / store / branch), so that cycles can be
# estimated from an assumed per-class latency table, run the 6-8 frame `small` build under Unicorn:
#   cargo build --release --features small
#   llvm-objcopy -O binary target/riscv32imc-unknown-none-elf/release/codec2_riscv32_bench bench.bin
#   python3 -m venv v && v/bin/pip install unicorn
#   v/bin/python mixcount.py bench.bin <addr of encode_profiled> <addr of decode_profiled> 8   # llvm-nm
# (the encode phase count also includes the harness's own text formatting, so use the class
# percentages, not the totals).
# Its checksum equals the host build's `cargo run --release --example codec2_fixed_bench --
# <wav> 1 100 200`, proving the 32-bit build is bit-identical.
# Copies the codec2_3200 fixed-point sources (non-test parts) into src/codec2_3200 with no_std edits.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
SRC=${1:-$HERE/../../src/codec2_3200}
D=$HERE/src/codec2_3200
FIX="$HERE/../../tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav"
# 200 frames of real speech starting 2 s in (frame 100), plus a 6-frame excerpt for the `small` build.
dd if="$FIX" of="$HERE/speech.raw" bs=1 skip=$((44+2*16000)) count=$((160*2*200)) 2>/dev/null
head -c $((320*8)) "$HERE/speech.raw" > "$HERE/speech_small.raw"
rm -rf $D; mkdir -p $D
for f in bits encoder_fixed envelope fixed_fft fixed_point interp lpc nlp quantise spectral_bridge synthesis tables trig_fixed voicing window mod; do
  L=$(grep -n -A1 '^#\[cfg(test)\]' $SRC/$f.rs | grep -B1 -- '-mod tests' | head -1 | cut -d: -f1)
  if [ -n "$L" ]; then head -n $((L-1)) $SRC/$f.rs; else cat $SRC/$f.rs; fi | \
  sed -e 's/std::sync::OnceLock/crate::shim::OnceLock/g' -e 's/use std::sync::OnceLock;/use crate::shim::OnceLock;/' \
      -e 's/std::f32::consts/core::f32::consts/g' -e 's/std::array/core::array/g' -e 's/std::f64::consts/core::f64::consts/g' \
      -e '$a #[allow(unused_imports)] use crate::shim::FloatMath as _;' > $D/$f.rs
done
sed -i -e '/pub mod floating_reference;/d' $D/mod.rs
sed -i 's/use rustfft::num_complex::Complex32;/use microfft::Complex32;/' $D/*.rs
cat $HERE/prof_enc.rs >> $D/encoder_fixed.rs
cat $HERE/prof_dec.rs >> $D/mod.rs
# Keep the big-frame functions out of line so the measured stack high-water mark reflects real call
# frames instead of everything being inlined into `main`.
sed -i 's/^pub fn nlp_fixed_bin(/#[inline(never)]\npub fn nlp_fixed_bin(/' $D/nlp.rs
sed -i 's/^pub(crate) fn compute_harmonic_amplitudes_fixed(/#[inline(never)]\npub(crate) fn compute_harmonic_amplitudes_fixed(/; s/^fn lpc_spectrum_fixed(/#[inline(never)]\nfn lpc_spectrum_fixed(/' $D/envelope.rs
sed -i 's/^    pub(crate) fn synthesize_subframe_fixed(/    #[inline(never)]\n    pub(crate) fn synthesize_subframe_fixed(/' $D/synthesis.rs
sed -i 's/^pub fn lpc_to_lsp_q23_from_integer_ak(/#[inline(never)]\npub fn lpc_to_lsp_q23_from_integer_ak(/; s/^pub fn autocorrelate_fixed(/#[inline(never)]\npub fn autocorrelate_fixed(/' $D/lpc.rs
# ---- optional fine-grained marks (bench only) ----
python3 - "$D" <<'PY'
import sys
D=sys.argv[1]
def patch(f, pairs):
    p=D+'/'+f; s=open(p).read()
    for a,b in pairs:
        if a not in s: print("MISSING", f, a[:50]); continue
        s=s.replace(a,b,1)
    open(p,'w').write(s)
patch('envelope.rs',[
 ("    let aw = lpc_spectrum_fixed(ak_q23);\n    let a2:", "    crate::prof::mark(15);\n    let aw = lpc_spectrum_fixed(ak_q23);\n    crate::prof::mark(6);\n    let a2:"),
 ("    let mut ak_gamma_q23 = [0i64; LPC_ORD + 1];\n", "    crate::prof::mark(7);\n    let mut ak_gamma_q23 = [0i64; LPC_ORD + 1];\n"),
 ("    let mut pw_bin_q23", "    crate::prof::mark(8);\n    let mut pw_bin_q23"),
])
patch('synthesis.rs',[
 ("        let h = super::envelope::sample_filter_phase_fixed(aw, model);\n        synthesize_phase_fixed(", "        crate::prof::mark(15);\n        let h = super::envelope::sample_filter_phase_fixed(aw, model);\n        synthesize_phase_fixed("),
 ("        postfilter_fixed(model, &mut self.bg_est, &mut self.rng);\n", "        crate::prof::mark(11);\n        postfilter_fixed(model, &mut self.bg_est, &mut self.rng);\n        crate::prof::mark(12);\n"),
 ("        fft_fixed(&mut self.ifft_re, &mut self.ifft_im, false);\n", "        crate::prof::mark(13);\n        fft_fixed(&mut self.ifft_re, &mut self.ifft_im, false);\n        crate::prof::mark(14);\n"),
])
PY
