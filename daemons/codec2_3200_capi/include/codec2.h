/*---------------------------------------------------------------------------*\

  FILE........: codec2.h
  DROP-IN FOR..: Codec2's own real upstream public header (David Rowe,
                 codec2.git src/codec2.h) -- function names, signatures,
                 and CODEC2_MODE_* values below are copied verbatim from
                 that real published API so a caller (e.g. an M17 stack)
                 that already includes <codec2.h> and links -lcodec2
                 doesn't need to change either its source or its link
                 line to use this implementation instead.

  SCOPE........: This header only declares the subset of the real API
                 this crate actually implements: create/destroy/encode/
                 decode/decode_ber and the three frame-size queries, for
                 CODEC2_MODE_3200 only (codec2_create returns NULL for
                 every other mode, exactly like the real library does
                 for a mode compiled out via -DCODEC2_MODE_x_EN=0). The
                 real header's other exported functions
                 (codec2_set_lpc_post_filter, codec2_get_spare_bit_index,
                 codec2_rebuild_spare_bit, codec2_set_natural_or_gray,
                 codec2_set_softdec, codec2_get_energy, the ML/VQ
                 experiment hooks, and the 700C-specific post filter/eq
                 functions) are deliberately NOT declared or exported
                 here -- a caller that needs one of those gets a real
                 link error, not a silent no-op standing in for a
                 feature this port doesn't have.

\*---------------------------------------------------------------------------*/

/*
  Implementation: SPDX-License-Identifier: LGPL-3.0-or-later
  (ham_digital_modes's own independent Codec2 3200bps port -- see
  src/codec2_3200/mod.rs's own module doc comment for how it was built
  and verified; not a derivative of Codec2-mod's LGPL-2.1-only source.)
*/

#ifndef __CODEC2__
#define __CODEC2__

#ifdef __cplusplus
extern "C" {
#endif

#define CODEC2_MODE_3200 0
/* Real upstream's other mode numbers, kept here only so a caller's own
   mode-selection code compiles unchanged -- codec2_create() returns
   NULL for every one of these, since this port only implements 3200. */
#define CODEC2_MODE_2400 1
#define CODEC2_MODE_1600 2
#define CODEC2_MODE_1400 3
#define CODEC2_MODE_1300 4
#define CODEC2_MODE_1200 5
#define CODEC2_MODE_700C 8

struct CODEC2;

struct CODEC2 *codec2_create(int mode);
void codec2_destroy(struct CODEC2 *codec2_state);
void codec2_encode(struct CODEC2 *codec2_state, unsigned char bytes[],
                    short speech_in[]);
void codec2_decode(struct CODEC2 *codec2_state, short speech_out[],
                    const unsigned char bytes[]);
void codec2_decode_ber(struct CODEC2 *codec2_state, short speech_out[],
                        const unsigned char *bytes, float ber_est);
int codec2_samples_per_frame(struct CODEC2 *codec2_state);
int codec2_bits_per_frame(struct CODEC2 *codec2_state);
int codec2_bytes_per_frame(struct CODEC2 *codec2_state);

#ifdef __cplusplus
}
#endif

#endif
