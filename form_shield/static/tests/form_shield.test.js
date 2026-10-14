/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { describe, expect, test } from "@odoo/hoot";
import { noticeText } from "@form_shield/js/error_notice";
import { ShieldSession, solve, fetchChallenge, CHALLENGE_URL, WORKER_URL } from "@form_shield/js/shield_client";

// The vector tests/test_tokens.py asserts against tokens.solve_pow(): the same challenge, form and
// difficulty must give the same smallest counter in the browser and in Python.
const VECTOR_TOKEN =
    "v1.pow.1000.1600.16.00112233aabbccdd.0123456789abcdef.0000000000000000000000000000000000000000";
const VECTOR_FORM = "contact";
const VECTOR_COUNTER = 23661;

describe("form_shield", () => {
    describe.current.tags("form_shield");

    // [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
    test("pow_core.js hashes and solves exactly like tokens.py", () => {
        // Tests [@ANCHOR: form_shield:pow_core_js]
        const core = globalThis.FormShieldPow;
        expect(core.hex(core.sha256Words("abc"))).toBe(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        expect(core.hex(core.sha256Words(""))).toBe(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // 55, 56, 63, 64 and 65 bytes straddle the padding boundaries.
        expect(core.hex(core.sha256Words("a".repeat(55)))).toBe(
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
        expect(core.hex(core.sha256Words("a".repeat(64)))).toBe(
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
        expect(core.solve(VECTOR_TOKEN, VECTOR_FORM, 16)).toBe(VECTOR_COUNTER);
        expect(core.leadingZeroBits(new Uint32Array([0, 0x00ffffff, 0, 0, 0, 0, 0, 0]))).toBe(40);
        // A different form gives a different answer.
        expect(core.solve(VECTOR_TOKEN, "other", 12)).not.toBe(core.solve(VECTOR_TOKEN, VECTOR_FORM, 12));
    });

    // [@ANCHOR: form_shield:test_pow_worker_runs_in_a_real_browser]
    test("the Web Worker file solves the vector in a real browser", async () => {
        // Tests [@ANCHOR: form_shield:pow_worker_js]
        const counter = await new Promise((resolve, reject) => {
            const worker = new Worker(WORKER_URL);
            worker.onmessage = (event) => {
                worker.terminate();
                resolve(event.data.counter);
            };
            worker.onerror = (event) => reject(new Error(event.message));
            worker.postMessage({ challenge: VECTOR_TOKEN, formId: VECTOR_FORM, difficulty: 16 });
        });
        expect(counter).toBe(VECTOR_COUNTER);
    });

    // [@ANCHOR: form_shield:test_client_session_solves_and_waits]
    test("ShieldSession fetches, solves in a worker and waits out the minimum age", async () => {
        // Tests [@ANCHOR: form_shield:client_session]
        const requests = [];
        const fetchImpl = async (url, options) => {
            requests.push([url, options.method, options.credentials]);
            return {
                ok: true,
                json: async () => ({ challenge: VECTOR_TOKEN, difficulty: 16, min_age: 3 }),
            };
        };
        const waits = [];
        const waitImpl = async (milliseconds) => waits.push(milliseconds);
        const session = new ShieldSession(VECTOR_FORM, { fetchImpl, waitImpl });
        const answer = await session.token();
        expect(answer).toBe(`${VECTOR_TOKEN}~${VECTOR_COUNTER}`);
        expect(requests).toEqual([[CHALLENGE_URL, "POST", "omit"]]);
        // 3 s minimum plus the margin, less however long the fetch and solve took.
        expect(waits.length).toBe(1);
        expect(waits[0] > 2000 && waits[0] <= 3400).toBe(true);
        // The form id may also be learned late, as Odoo's own forms do.
        const late = new ShieldSession(null, { fetchImpl, waitImpl });
        expect(await late.token(VECTOR_FORM)).toBe(`${VECTOR_TOKEN}~${VECTOR_COUNTER}`);
    });

    test("a server error is an unavailable shield, not a silent pass", async () => {
        // Tests [@ANCHOR: form_shield:client_session]
        const fetchImpl = async () => ({ ok: false, status: 503 });
        let failure = null;
        try {
            await fetchChallenge("pow", fetchImpl);
        } catch (error) {
            failure = error;
        }
        expect(failure.constructor.name).toBe("ShieldUnavailable");
        const session = new ShieldSession(VECTOR_FORM, { fetchImpl, waitImpl: async () => {} });
        let rejected = false;
        await session.token().catch(() => {
            rejected = true;
        });
        expect(rejected).toBe(true);
    });

    // [@ANCHOR: form_shield:test_error_notice_messages]
    test("every refusal a person can meet has its own words", () => {
        // Tests [@ANCHOR: form_shield:error_notice]
        const reasons = ["expired", "ip_changed", "too_fast", "wrong_answer", "no_token", "rate_subject", "rate_target", "rate_global", "unavailable"];
        const texts = new Set(reasons.map((reason) => noticeText(reason)));
        expect(texts.size).toBe(reasons.length);
        expect(noticeText("something new")).toBe("We could not send the form. Please try again.");
    });

    test("difficulty 0 needs no work", async () => {
        expect(await solve(VECTOR_TOKEN, VECTOR_FORM, 0)).toBe(0);
    });
});
