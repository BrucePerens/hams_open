/* SPDX-License-Identifier: LGPL-3.0-or-later
 *
 * Real link-level proof the C ABI in ../src/lib.rs is actually usable
 * from C, not just from the Rust-side FFI unit tests in that same
 * crate: this program #includes the real drop-in header
 * (../include/codec2.h) and links against the actual libcodec2.so/.a
 * this crate builds, the same way an M17 stack's own build would, then
 * calls create/encode/decode/destroy through it.
 *
 * Build (from this crate's own directory, after `cargo build`):
 *   cc tests/c_link_smoke_test.c -Iinclude -Ltarget/debug -lcodec2 \
 *      -Wl,-rpath,target/debug -o /tmp/c_link_smoke_test
 *   /tmp/c_link_smoke_test
 */

#include "codec2.h"
#include <stdio.h>
#include <stdlib.h>
#include <math.h>

int main(void) {
    /* Mirrors the crate's own Rust-side FFI test: two independent
     * handles (an M17 TX chain and RX chain would each own one), a
     * synthetic tone encoded then decoded across several 20ms frames,
     * checked for a clean run and finite, non-clipping output -- not
     * exact sample values (the Rust decoder-vs-reference regression
     * test already covers real fidelity numerically). */
    struct CODEC2 *enc = codec2_create(CODEC2_MODE_3200);
    struct CODEC2 *dec = codec2_create(CODEC2_MODE_3200);
    if (!enc || !dec) {
        fprintf(stderr, "codec2_create failed\n");
        return 1;
    }

    int n_samp = codec2_samples_per_frame(enc);
    int n_bytes = codec2_bytes_per_frame(enc);
    if (n_samp != 160 || n_bytes != 8 || codec2_bits_per_frame(enc) != 64) {
        fprintf(stderr, "unexpected frame sizes: samples=%d bytes=%d bits=%d\n",
                n_samp, n_bytes, codec2_bits_per_frame(enc));
        return 1;
    }

    short speech_in[160];
    unsigned char bytes[8];
    short speech_out[160];

    for (int frame = 0; frame < 50; frame++) {
        for (int i = 0; i < n_samp; i++) {
            double t = (double)(frame * n_samp + i) / 8000.0;
            speech_in[i] = (short)(3000.0 * sin(2.0 * M_PI * 200.0 * t));
        }

        codec2_encode(enc, bytes, speech_in);
        codec2_decode(dec, speech_out, bytes);

        for (int i = 0; i < n_samp; i++) {
            if (speech_out[i] <= -32767 || speech_out[i] >= 32767) {
                fprintf(stderr, "sample clipped on frame %d: %d\n", frame, speech_out[i]);
                return 1;
            }
        }
    }

    /* Also exercise codec2_decode_ber directly (the real M17-relevant
     * soft-decision entry point some callers use instead of plain
     * codec2_decode) -- confirmed in the Rust side that ber_est is
     * unused for this mode, but the call itself must still work. */
    codec2_decode_ber(dec, speech_out, bytes, 0.05f);

    codec2_destroy(enc);
    codec2_destroy(dec);

    printf("c_link_smoke_test: OK (%d frames, %d samples/frame, %d bytes/frame)\n", 50, n_samp, n_bytes);
    return 0;
}
