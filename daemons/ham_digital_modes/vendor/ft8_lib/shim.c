// SPDX-License-Identifier: LGPL-3.0-or-later
// Thin C wrapper over ft8_lib (vendored, MIT-licensed -- see LICENSE-MIT
// in this directory) giving Rust an opaque-pointer, allocation-and-
// struct-layout-free API. Every ft8_lib struct with a non-trivial layout
// (monitor_t, ftx_waterfall_t, the WATERFALL_USE_PHASE-conditional
// fields) stays entirely on the C side -- Rust never constructs or
// reads one directly, only holds an opaque `Ft8Session*`. This is the
// standard, safe pattern for binding a C library with structs whose
// exact layout isn't worth keeping in sync by hand across languages.
//
// The decode-and-format pipeline (candidate search -> LDPC decode ->
// hash-table dedup -> message-to-text) is a faithful port of ft8_lib's
// own demo/decode_ft8.c `decode()` function and its callsign hashtable
// (hashtable_add/hashtable_lookup) -- not a reimplementation from the
// header comments, the actual reference driver's logic, made
// per-session instead of global/static so multiple decode sessions
// (e.g. more than one radio/band) can run without sharing state.

#include <stdlib.h>
#include <string.h>

#include <ft8/decode.h>
#include <ft8/message.h>
#include <common/monitor.h>

#define FT8_MIN_SCORE 10
#define FT8_MAX_CANDIDATES 140
#define FT8_LDPC_ITERATIONS 25
#define FT8_MAX_DECODED_PER_CALL 50
#define FT8_CALLSIGN_HASHTABLE_SIZE 256

typedef struct {
    char callsign[12];
    uint32_t hash;
} callsign_hash_entry_t;

typedef struct {
    monitor_t mon;
    ftx_message_t decoded[FT8_MAX_DECODED_PER_CALL];
    ftx_message_t* decoded_hashtable[FT8_MAX_DECODED_PER_CALL];
} ft8_session_t;

// ft8_lib's ftx_callsign_hash_interface_t callbacks take no context/user
// data parameter (confirmed against the vendored message.h -- just
// `(const char*, uint32_t)`/`(hash_type, hash, char*)`), so this table
// can't be made per-session without changing that upstream signature.
// Matches decode_ft8.c's own real reference implementation exactly: one
// process-wide table. Known, deliberate limitation: if this daemon ever
// runs more than one concurrent FT8 decode session, their hashed-
// callsign resolution (only relevant to continuation messages that
// reference an earlier-seen callsign by its hash, not most CQ/initial
// messages) would share and could cross-pollute this table. The core
// sync/LDPC/CRC decode of every message is unaffected either way.
static callsign_hash_entry_t g_callsign_hashtable[FT8_CALLSIGN_HASHTABLE_SIZE];

static bool global_hash_lookup(ftx_callsign_hash_type_t hash_type, uint32_t hash, char* callsign) {
    uint8_t hash_shift = (hash_type == FTX_CALLSIGN_HASH_10_BITS) ? 12 : (hash_type == FTX_CALLSIGN_HASH_12_BITS ? 10 : 0);
    uint16_t hash10 = (hash >> (12 - hash_shift)) & 0x3FFu;
    int idx = (hash10 * 23) % FT8_CALLSIGN_HASHTABLE_SIZE;
    while (g_callsign_hashtable[idx].callsign[0] != '\0') {
        if (((g_callsign_hashtable[idx].hash & 0x3FFFFFu) >> hash_shift) == hash) {
            strcpy(callsign, g_callsign_hashtable[idx].callsign);
            return true;
        }
        idx = (idx + 1) % FT8_CALLSIGN_HASHTABLE_SIZE;
    }
    callsign[0] = '\0';
    return false;
}

static void global_hash_save(const char* callsign, uint32_t hash) {
    uint16_t hash10 = (hash >> 12) & 0x3FFu;
    int idx = (hash10 * 23) % FT8_CALLSIGN_HASHTABLE_SIZE;
    while (g_callsign_hashtable[idx].callsign[0] != '\0') {
        if (((g_callsign_hashtable[idx].hash & 0x3FFFFFu) == hash) && (0 == strcmp(g_callsign_hashtable[idx].callsign, callsign))) {
            g_callsign_hashtable[idx].hash &= 0x3FFFFFu;
            return;
        }
        idx = (idx + 1) % FT8_CALLSIGN_HASHTABLE_SIZE;
    }
    strncpy(g_callsign_hashtable[idx].callsign, callsign, 11);
    g_callsign_hashtable[idx].callsign[11] = '\0';
    g_callsign_hashtable[idx].hash = hash;
}

// Allocates and initializes a new decode session. sample_rate is the
// input audio sample rate (Hz); f_min/f_max bound the analysis
// frequency range in Hz. Returns NULL on allocation failure.
ft8_session_t* ft8_session_new(int sample_rate, float f_min, float f_max) {
    ft8_session_t* s = (ft8_session_t*)calloc(1, sizeof(ft8_session_t));
    if (!s) return NULL;
    monitor_config_t cfg = {
        .f_min = f_min,
        .f_max = f_max,
        .sample_rate = sample_rate,
        .time_osr = 2,
        .freq_osr = 2,
        .protocol = FTX_PROTOCOL_FT8,
    };
    monitor_init(&s->mon, &cfg);
    return s;
}

void ft8_session_free(ft8_session_t* s) {
    if (!s) return;
    monitor_free(&s->mon);
    free(s);
}

// Feeds one analysis block's worth of audio (mon.block_size samples,
// see ft8_session_block_size below) into the waterfall accumulator.
// Call this repeatedly (once per block, with 50% overlap handled
// internally by ft8_lib) until a full ~15s FT8 slot has accumulated,
// then call ft8_session_decode.
void ft8_session_process(ft8_session_t* s, const float* frame) {
    monitor_process(&s->mon, frame);
}

int ft8_session_block_size(const ft8_session_t* s) {
    return s->mon.block_size;
}

void ft8_session_reset(ft8_session_t* s) {
    monitor_reset(&s->mon);
}

// Attempts to decode every candidate currently in the accumulated
// waterfall. Writes up to max_messages decoded, human-readable message
// strings (each up to FTX_MAX_MESSAGE_LENGTH bytes, caller-allocated)
// into out_messages, and their integer SNR estimates into out_snr.
// Returns the number of messages actually decoded (may be 0).
int ft8_session_decode(ft8_session_t* s, char out_messages[][FTX_MAX_MESSAGE_LENGTH], int* out_snr, int max_messages) {
    const ftx_waterfall_t* wf = &s->mon.wf;
    ftx_candidate_t candidate_list[FT8_MAX_CANDIDATES];
    int num_candidates = ftx_find_candidates(wf, FT8_MAX_CANDIDATES, candidate_list, FT8_MIN_SCORE);

    ftx_callsign_hash_interface_t hash_if = {
        .lookup_hash = global_hash_lookup,
        .save_hash = global_hash_save,
    };

    int num_decoded = 0;
    for (int i = 0; i < FT8_MAX_DECODED_PER_CALL; ++i) {
        s->decoded_hashtable[i] = NULL;
    }

    int out_count = 0;
    for (int idx = 0; idx < num_candidates && out_count < max_messages; ++idx) {
        const ftx_candidate_t* cand = &candidate_list[idx];

        ftx_message_t message;
        ftx_decode_status_t status;
        if (!ftx_decode_candidate(wf, cand, FT8_LDPC_ITERATIONS, &message, &status)) {
            continue;
        }

        int idx_hash = message.hash % FT8_MAX_DECODED_PER_CALL;
        bool found_empty_slot = false;
        bool found_duplicate = false;
        do {
            if (s->decoded_hashtable[idx_hash] == NULL) {
                found_empty_slot = true;
            } else if ((s->decoded_hashtable[idx_hash]->hash == message.hash) &&
                       (0 == memcmp(s->decoded_hashtable[idx_hash]->payload, message.payload, sizeof(message.payload)))) {
                found_duplicate = true;
            } else {
                idx_hash = (idx_hash + 1) % FT8_MAX_DECODED_PER_CALL;
            }
        } while (!found_empty_slot && !found_duplicate);

        if (found_empty_slot) {
            memcpy(&s->decoded[idx_hash], &message, sizeof(message));
            s->decoded_hashtable[idx_hash] = &s->decoded[idx_hash];
            ++num_decoded;

            ftx_message_offsets_t offsets;
            char text[FTX_MAX_MESSAGE_LENGTH];
            ftx_message_rc_t rc = ftx_message_decode(&message, &hash_if, text, &offsets);
            if (rc != FTX_MESSAGE_RC_OK) {
                continue; // couldn't unpack to text (e.g. an unresolved hashed callsign); skip rather than emit garbage
            }
            strncpy(out_messages[out_count], text, FTX_MAX_MESSAGE_LENGTH - 1);
            out_messages[out_count][FTX_MAX_MESSAGE_LENGTH - 1] = '\0';
            out_snr[out_count] = cand->score;
            ++out_count;
        }
    }
    return out_count;
}
