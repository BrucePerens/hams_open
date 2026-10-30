/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * [@ANCHOR: form_shield:pow_core_js]
 * Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
 *
 * The proof-of-work solver, in plain script form (not an Odoo module) so one file serves both the
 * Web Worker (importScripts) and the page bundle (main-thread fallback for a browser without
 * workers), and so the hoot test loads exactly what ships.
 *
 * The hash is SHA-256 over the ASCII bytes of  <challenge>:<form_id>:<counter>  with the counter
 * in decimal from 0; the answer is the SMALLEST counter whose digest starts with `difficulty` zero
 * bits. tokens.py (pow_digest/solve_pow) computes the same thing; tests/test_tokens.py and
 * form_shield.test.js assert the same vector. The SHA-256 is written out here because
 * crypto.subtle is asynchronous and far too slow to call a million times.
 */
(function (root) {
    "use strict";

    const K = new Uint32Array([
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ]);
    const INITIAL = new Uint32Array([
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ]);
    const W = new Uint32Array(64);

    // [@ANCHOR: form_shield:pow_rotr]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function rotr(value, bits) {
        return (value >>> bits) | (value << (32 - bits));
    }

    /* Folds the 64-byte block at bytes[offset..] into `state` (an Uint32Array(8)). */
    // [@ANCHOR: form_shield:pow_compress]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function compress(state, bytes, offset) {
        for (let i = 0; i < 16; i++) {
            const j = offset + i * 4;
            W[i] = (bytes[j] << 24) | (bytes[j + 1] << 16) | (bytes[j + 2] << 8) | bytes[j + 3];
        }
        for (let i = 16; i < 64; i++) {
            const w15 = W[i - 15];
            const w2 = W[i - 2];
            const s0 = rotr(w15, 7) ^ rotr(w15, 18) ^ (w15 >>> 3);
            const s1 = rotr(w2, 17) ^ rotr(w2, 19) ^ (w2 >>> 10);
            W[i] = (W[i - 16] + s0 + W[i - 7] + s1) | 0;
        }
        let a = state[0], b = state[1], c = state[2], d = state[3];
        let e = state[4], f = state[5], g = state[6], h = state[7];
        for (let i = 0; i < 64; i++) {
            const big1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
            const choose = (e & f) ^ (~e & g);
            const t1 = (h + big1 + choose + K[i] + W[i]) | 0;
            const big0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
            const majority = (a & b) ^ (a & c) ^ (b & c);
            const t2 = (big0 + majority) | 0;
            h = g; g = f; f = e; e = (d + t1) | 0;
            d = c; c = b; b = a; a = (t1 + t2) | 0;
        }
        state[0] = (state[0] + a) | 0; state[1] = (state[1] + b) | 0;
        state[2] = (state[2] + c) | 0; state[3] = (state[3] + d) | 0;
        state[4] = (state[4] + e) | 0; state[5] = (state[5] + f) | 0;
        state[6] = (state[6] + g) | 0; state[7] = (state[7] + h) | 0;
    }

    // [@ANCHOR: form_shield:pow_ascii_bytes]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function asciiBytes(text) {
        const bytes = new Uint8Array(text.length);
        for (let i = 0; i < text.length; i++) {
            bytes[i] = text.charCodeAt(i) & 0x7f;
        }
        return bytes;
    }

    /* SHA-256 of an ASCII string, as 8 big-endian words (Uint32Array). For tests and small uses. */
    // [@ANCHOR: form_shield:pow_sha256_words]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function sha256Words(text) {
        const message = asciiBytes(text);
        const blocks = Math.ceil((message.length + 9) / 64);
        const padded = new Uint8Array(blocks * 64);
        padded.set(message);
        padded[message.length] = 0x80;
        const bits = message.length * 8;
        padded[padded.length - 4] = (bits >>> 24) & 0xff;
        padded[padded.length - 3] = (bits >>> 16) & 0xff;
        padded[padded.length - 2] = (bits >>> 8) & 0xff;
        padded[padded.length - 1] = bits & 0xff;
        const state = new Uint32Array(INITIAL);
        for (let block = 0; block < blocks; block++) {
            compress(state, padded, block * 64);
        }
        return state;
    }

    // [@ANCHOR: form_shield:pow_hex]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function hex(words) {
        let out = "";
        for (const word of words) {
            out += (word >>> 0).toString(16).padStart(8, "0");
        }
        return out;
    }

    // [@ANCHOR: form_shield:pow_leading_zero_bits]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function leadingZeroBits(words) {
        let bits = 0;
        for (let i = 0; i < 8; i++) {
            const word = words[i] >>> 0;
            if (word === 0) {
                bits += 32;
                continue;
            }
            return bits + Math.clz32(word);
        }
        return bits;
    }

    /*
     * A resumable solver. step(count) tries up to `count` more counters and returns the winning
     * counter, or null when none of them wins; call it again to continue. The part of the message
     * before the counter is hashed once; each trial only redoes the last one or two blocks.
     */
    // [@ANCHOR: form_shield:pow_create_solver]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function createSolver(challenge, formId, difficulty) {
        const prefix = asciiBytes(challenge + ":" + formId + ":");
        const midstate = new Uint32Array(INITIAL);
        const wholeBlocks = Math.floor(prefix.length / 64);
        for (let block = 0; block < wholeBlocks; block++) {
            compress(midstate, prefix, block * 64);
        }
        const rest = prefix.subarray(wholeBlocks * 64);
        const tail = new Uint8Array(128);
        const trial = new Uint32Array(8);
        let counter = 0;

        function digestOf(value) {
            const digits = String(value);
            let length = rest.length;
            tail.fill(0);
            tail.set(rest);
            for (let i = 0; i < digits.length; i++) {
                tail[length++] = digits.charCodeAt(i);
            }
            tail[length] = 0x80;
            const total = length + 1 + 8 <= 64 ? 64 : 128;
            const bits = (wholeBlocks * 64 + length) * 8;
            tail[total - 4] = (bits >>> 24) & 0xff;
            tail[total - 3] = (bits >>> 16) & 0xff;
            tail[total - 2] = (bits >>> 8) & 0xff;
            tail[total - 1] = bits & 0xff;
            trial.set(midstate);
            compress(trial, tail, 0);
            if (total === 128) {
                compress(trial, tail, 64);
            }
            return trial;
        }

        return {
            step(count) {
                for (let n = 0; n < count; n++) {
                    const words = digestOf(counter);
                    if (leadingZeroBits(words) >= difficulty) {
                        return counter;
                    }
                    counter++;
                }
                return null;
            },
            get tried() {
                return counter;
            },
        };
    }

    /* Blocking solve; the worker and the tests use this. */
    // [@ANCHOR: form_shield:pow_solve]
    // Verified by [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    function solve(challenge, formId, difficulty) {
        const solver = createSolver(challenge, formId, difficulty);
        for (;;) {
            const found = solver.step(50000);
            if (found !== null) {
                return found;
            }
        }
    }

    root.FormShieldPow = { sha256Words, hex, leadingZeroBits, createSolver, solve };
})(typeof self !== "undefined" ? self : globalThis);
