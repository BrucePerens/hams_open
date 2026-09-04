/* SPDX-License-Identifier: LGPL-3.0-or-later
 *
 * The real interop proof the loopback smoke test (c_link_smoke_test.c)
 * doesn't give: decodes a real bitstream -- captured directly from the
 * unmodified real Codec2-mod C *encoder*, not this port's own encoder,
 * so this is fully independent of any of this crate's own encoder-side
 * choices -- through the actual C ABI's own `codec2_decode`, and
 * compares against that same real bitstream's real reference-decoder
 * PCM output (also captured from the unmodified real C decoder). Both
 * fixture files are the same ones
 * `ham_digital_modes/src/codec2_3200/mod.rs`'s own
 * `decoder_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream`
 * Rust-level test already uses and validates at a >0.99 correlation
 * threshold -- this test exists specifically to prove the *C ABI*
 * itself doesn't lose or corrupt anything relative to that Rust-level
 * result (a wrapper bug -- wrong byte count, wrong pointer stride --
 * would show up here even if the underlying Rust decoder is correct).
 *
 * Build (from this crate's own directory, after `cargo build`):
 *   cc tests/c_link_reference_bitstream_test.c -Iinclude -Ltarget/debug \
 *      -lcodec2 -Wl,-rpath,target/debug -lm \
 *      -o /tmp/c_link_reference_bitstream_test
 *   /tmp/c_link_reference_bitstream_test \
 *      ../ham_digital_modes/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin \
 *      ../ham_digital_modes/tests/fixtures/codec2_3200/synthetic_c_decoded_pcm.bin
 */

#include "codec2.h"
#include <stdio.h>
#include <stdlib.h>
#include <math.h>

static unsigned char *read_file(const char *path, long *out_len) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        fprintf(stderr, "cannot open %s\n", path);
        exit(1);
    }
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char *buf = malloc((size_t)len);
    if (fread(buf, 1, (size_t)len, f) != (size_t)len) {
        fprintf(stderr, "short read on %s\n", path);
        exit(1);
    }
    fclose(f);
    *out_len = len;
    return buf;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s <encoded_bits.bin> <reference_decoded_pcm.bin>\n", argv[0]);
        return 1;
    }

    long bits_len, pcm_len;
    unsigned char *bits_buf = read_file(argv[1], &bits_len);
    unsigned char *pcm_buf = read_file(argv[2], &pcm_len);

    struct CODEC2 *dec = codec2_create(CODEC2_MODE_3200);
    if (!dec) {
        fprintf(stderr, "codec2_create failed\n");
        return 1;
    }
    int n_bytes = codec2_bytes_per_frame(dec);
    int n_samp = codec2_samples_per_frame(dec);

    long n_frames = bits_len / n_bytes;
    long expected_samples = n_frames * n_samp;
    long reference_samples = pcm_len / 2; /* int16 */
    if (expected_samples != reference_samples) {
        fprintf(stderr, "frame-count mismatch: bitstream implies %ld samples, reference PCM has %ld\n",
                expected_samples, reference_samples);
        return 1;
    }

    short *decoded = malloc((size_t)expected_samples * sizeof(short));
    for (long f = 0; f < n_frames; f++) {
        codec2_decode(dec, decoded + f * n_samp, bits_buf + f * n_bytes);
    }
    codec2_destroy(dec);

    /* Pearson correlation between this C-ABI decode and the real
     * reference decoder's own output on the identical bitstream. */
    double mean_a = 0.0, mean_b = 0.0;
    const short *ref = (const short *)pcm_buf;
    for (long i = 0; i < expected_samples; i++) {
        mean_a += decoded[i];
        mean_b += ref[i];
    }
    mean_a /= (double)expected_samples;
    mean_b /= (double)expected_samples;

    double cov = 0.0, var_a = 0.0, var_b = 0.0;
    for (long i = 0; i < expected_samples; i++) {
        double da = decoded[i] - mean_a;
        double db = ref[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    double corr = cov / sqrt(var_a * var_b);

    printf("c_link_reference_bitstream_test: %ld frames, %ld samples, correlation=%.6f\n",
           n_frames, expected_samples, corr);

    if (corr <= 0.99) {
        fprintf(stderr, "FAILED: correlation %.6f did not exceed 0.99\n", corr);
        return 1;
    }
    printf("PASSED\n");
    return 0;
}
