# codec2_3200_capi

A C ABI drop-in layer over `ham_digital_modes`'s independent Codec2
3200bps port (`daemons/ham_digital_modes/src/codec2_3200/`), so it can
replace real Codec2's own `libcodec2` at link time for a C/C++ caller
that only needs `CODEC2_MODE_3200`.

Why this exists: `ham_digital_modes`'s Codec2 3200bps implementation is
an independently-authored, from-scratch Rust port (see that module's
own doc comment), written specifically so this project's LGPL-3.0-or-
later code isn't a derivative work of the vendored real Codec2-mod
source, which is LGPL-2.1-only (no "or later" grant, not automatically
combinable). That independent implementation is bitstream-compatible
with, and cross-validated against, the real reference -- see the module
doc comment for the actual measured numbers. This crate is what turns
that Rust module into something a C build can link against exactly like
the real library, without changing the caller's own source or link
flags.

**Where this does and doesn't connect to M17 specifically, checked
directly against real dependency source rather than assumed**: the M17
Rust toolkit this codebase previously scoped (`thombles/m17rt`, see
`hams_com/docs/proposals/blocked/M17_IMPLEMENTATION_PLAN.md` -- still
shelved on `m17rt`'s own maintenance-risk grounds, unrelated to any of
this) has its `m17codec2` crate depend on `codec2 = "0.3.0"`
(`scriptjunkie/codec2` on crates.io) -- a **pure-Rust** Codec2
implementation, not a C-library binding, and it's dual-licensed
**LGPL-2.1-only AND MIT**, so `m17codec2` already has an MIT escape
hatch from the LGPL question that plan doc flagged as unresolved,
independent of anything built here. This C ABI layer is real, tested,
general-purpose infrastructure regardless (any raw-C M17/Codec2 tooling
-- e.g. cross-validating against the real GPL-licensed C reference
tools for testing purposes -- can link against it), but it is *not*
what `m17codec2` itself would link against; a caller that specifically
wants this port to serve `m17codec2`'s own dependency slot would need a
separate Rust-API-compatible crate matching `scriptjunkie/codec2`'s
surface (`Codec2::new`/`encode`/`decode`), not this one -- and that's a
materially bigger, mode-broader effort (that crate implements six
Codec2 modes; this port implements one) not started here, since it
would be built specifically toward the `m17rt` adoption question the
M17 plan doc explicitly reserves for Bruce's own decision.

## What it provides

- `libcodec2.so` / `libcodec2.a` (both built by `cargo build`, the `[lib]
  name = "codec2"` in `Cargo.toml` -- deliberately the same name as the
  real library's own output, so an existing `-lcodec2` link line keeps
  working unchanged).
- `include/codec2.h`, matching the real upstream `codec2.h`'s own
  function signatures and `CODEC2_MODE_*` values for the subset this
  crate implements.

## Scope: what's implemented, and what deliberately isn't

Implemented, matching real Codec2's own documented behavior exactly
(checked directly against the real `codec2.c` source, not guessed):

- `codec2_create` / `codec2_destroy`
- `codec2_encode` / `codec2_decode` / `codec2_decode_ber`
- `codec2_samples_per_frame` / `codec2_bits_per_frame` /
  `codec2_bytes_per_frame`

`codec2_create` returns a working handle **only** for `CODEC2_MODE_3200`
(mode `0`) -- every other mode returns `NULL`, exactly like the real
library does for a mode compiled out via `-DCODEC2_MODE_x_EN=0`. This
port doesn't implement any other Codec2 mode, so that's not a shortcut,
it's the honest answer for what this build actually supports.

**Not implemented, and not declared in `include/codec2.h`** --
`codec2_set_lpc_post_filter`, `codec2_get_spare_bit_index`,
`codec2_rebuild_spare_bit`, `codec2_set_natural_or_gray`,
`codec2_set_softdec`, `codec2_get_energy`, the ML/VQ experiment hooks
(`codec2_open_mlfeat`, `codec2_load_codebook`, `codec2_get_var`,
`codec2_enable_user_ratek`), and the 700C-specific post filter/eq
functions. A caller that references one of these gets a real link
error against this library, not a silently-wrong no-op standing in for
a feature this port doesn't have. None of these are needed for the
basic encode/decode path.

**M17's own protocol uses more than one Codec2 mode** (3200bps for
pure-voice frames, 1600bps for its combined voice+data stream type, by
the protocol's own design) -- this library only covers 3200. A caller
that needs 1600 gets `NULL` from `codec2_create(CODEC2_MODE_1600)`, the
same honest "not implemented" answer as every other unimplemented mode,
not a silent wrong decode.

**Panics abort the process, not the caller's error handling.** Real
Codec2's own `assert()`-based checks compile out entirely under
`NDEBUG` (a typical release build), so a caller passing a bad pointer
there gets undefined behavior rather than a clean failure either way.
This crate's `assert!` guards are always on (Rust doesn't strip them by
default), which is a real behavior difference worth knowing before
linking this in: a caller bug that would silently corrupt memory
against the real library instead cleanly aborts the whole process
against this one -- safer, but a different failure mode a caller
relying on `NDEBUG`-stripped asserts for its own error recovery would
need to know about.

## Building and using

```sh
cd daemons/codec2_3200_capi
cargo build --release   # -> target/release/libcodec2.{so,a}
```

Link a C/C++ program against it the same way you'd link the real
library:

```sh
cc your_program.c -I daemons/codec2_3200_capi/include \
   -L daemons/codec2_3200_capi/target/release -lcodec2 \
   -Wl,-rpath,daemons/codec2_3200_capi/target/release \
   -o your_program
```

or statically:

```sh
cc your_program.c -I daemons/codec2_3200_capi/include \
   daemons/codec2_3200_capi/target/release/libcodec2.a \
   -lm -lpthread -ldl -o your_program
```

## Tests

- `tests/c_link_smoke_test.c` proves the C ABI runs cleanly end to end
  (create/encode/decode/decode_ber/destroy through the real linked
  library, both dynamic and static), but on its own can't prove
  correctness: an encode-then-decode-with-itself loopback would still
  "pass" even if the wrapper mangled bytes, since both sides would be
  wrong the same way.
- `tests/c_link_reference_bitstream_test.c` is the real interop proof:
  it decodes `ham_digital_modes/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin`
  -- a real bitstream captured directly from the unmodified real
  Codec2-mod C *encoder*, independent of anything this port's own
  encoder chose -- through this crate's actual `codec2_decode`, and
  checks the result against `synthetic_c_decoded_pcm.bin` (the same
  bitstream decoded by the real unmodified C decoder). Verified to
  reproduce the underlying Rust-level decoder test's own result
  (correlation 0.998674, run 2026-09-04) and to genuinely discriminate
  a real wrapper bug -- confirmed via negative control: corrupting the
  input bitstream drops the correlation to ~0.02, correctly failing the
  same `>0.99` threshold `ham_digital_modes`'s own
  `decoder_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream`
  Rust test uses.

```sh
cc tests/c_link_smoke_test.c -Iinclude -Ltarget/release -lcodec2 \
   -Wl,-rpath,"$(pwd)/target/release" -lm -o /tmp/c_link_smoke_test
/tmp/c_link_smoke_test

cc tests/c_link_reference_bitstream_test.c -Iinclude -Ltarget/release \
   -lcodec2 -Wl,-rpath,"$(pwd)/target/release" -lm \
   -o /tmp/c_link_reference_bitstream_test
/tmp/c_link_reference_bitstream_test \
   ../ham_digital_modes/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin \
   ../ham_digital_modes/tests/fixtures/codec2_3200/synthetic_c_decoded_pcm.bin
```
