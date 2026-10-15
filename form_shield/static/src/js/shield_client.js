/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

// pow_core.js is a plain script that sets globalThis.FormShieldPow; importing it here makes the loader run it
// before this module, in the page bundle and the hoot bundle alike.
import "@form_shield/pow/pow_core";

// [@ANCHOR: form_shield:client_session]
// Verified by [@ANCHOR: form_shield:test_client_session_solves_and_waits]
//
// The browser half of the form shield: fetch a challenge, solve the proof of work in a Web Worker
// (or on the main thread when the browser has none or the worker file cannot start), and hand the
// form a `<challenge>~<counter>` answer once the server's minimum time-to-submit has passed.
// Self-hosted: nothing here loads a script or contacts a host other than this site's.

export const CHALLENGE_URL = "/form_shield/challenge";
export const WORKER_URL = "/form_shield/static/src/pow/pow_worker.js";
// A little past the server's minimum age, so clock rounding never makes a fast machine too early.
const AGE_MARGIN_MS = 400;

export class ShieldUnavailable extends Error {}

export async function fetchChallenge(kind = "pow", fetchImpl = globalThis.fetch.bind(globalThis)) {
    const body = new URLSearchParams({ kind });
    const response = await fetchImpl(CHALLENGE_URL, {
        method: "POST",
        body,
        credentials: "same-origin",
        cache: "no-store",
    });
    if (!response.ok) {
        throw new ShieldUnavailable(`${CHALLENGE_URL} answered ${response.status}`);
    }
    return response.json();
}

export function solveOnMainThread(challenge, formId, difficulty) {
    const core = globalThis.FormShieldPow;
    if (!core) {
        return Promise.reject(new ShieldUnavailable("pow_core.js is not loaded"));
    }
    const solver = core.createSolver(challenge, formId, difficulty);
    return new Promise((resolve) => {
        const slice = () => {
            const found = solver.step(20000);
            if (found !== null) {
                resolve(found);
            } else {
                setTimeout(slice, 0);
            }
        };
        slice();
    });
}

function solveInWorker(challenge, formId, difficulty) {
    return new Promise((resolve, reject) => {
        let worker;
        try {
            worker = new Worker(WORKER_URL);
        } catch (error) {
            reject(error);
            return;
        }
        worker.onmessage = (event) => {
            worker.terminate();
            if (event.data.error) {
                reject(new ShieldUnavailable(event.data.error));
            } else {
                resolve(event.data.counter);
            }
        };
        worker.onerror = (event) => {
            worker.terminate();
            reject(new ShieldUnavailable(event.message || "worker failed"));
        };
        worker.postMessage({ challenge, formId, difficulty });
    });
}

export function solve(challenge, formId, difficulty) {
    if (!challenge || difficulty === 0) {
        return Promise.resolve(0);
    }
    if (typeof Worker === "undefined") {
        return solveOnMainThread(challenge, formId, difficulty);
    }
    return solveInWorker(challenge, formId, difficulty).catch(() =>
        solveOnMainThread(challenge, formId, difficulty)
    );
}

const defaultWait = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

export class ShieldSession {
    /**
     * @param {string|null} formId the form the answer is for, when known up front (then the
     *   solving starts at once); null to learn it at token() time.
     * @param {Object} [options] fetchImpl, waitImpl and solveImpl replace fetch(), the time-to-submit
     *   delay and the Web Worker solve; for tests of this class only, production passes nothing.
     */
    constructor(formId = null, { fetchImpl, waitImpl, solveImpl } = {}) {
        this.solveImpl = solveImpl || solve;
        this.formId = formId;
        this.fetchImpl = fetchImpl;
        this.wait = waitImpl || defaultWait;
        this.start();
    }

    start() {
        this.fetchedAt = null;
        this.challenge = fetchChallenge("pow", this.fetchImpl).then((issued) => {
            this.fetchedAt = performance.now();
            return issued;
        });
        // A rejection nobody has awaited yet must not be reported as unhandled; token() awaits it.
        this.challenge.catch(() => {});
        this.solution = null;
        if (this.formId) {
            this.solution = this._solveFor(this.formId);
            this.solution.catch(() => {});
        }
    }

    async _solveFor(formId) {
        const issued = await this.challenge;
        return this.solveImpl(issued.challenge, formId, issued.difficulty);
    }

    /** The `<challenge>~<counter>` answer for `formId`; throws ShieldUnavailable on failure. */
    async token(formId = this.formId) {
        const issued = await this.challenge;
        const solution =
            formId === this.formId && this.solution ? this.solution : this._solveFor(formId);
        const counter = await solution;
        const ageMs = performance.now() - this.fetchedAt;
        await this.wait(Math.max(0, issued.min_age * 1000 + AGE_MARGIN_MS - ageMs));
        return `${issued.challenge}~${counter}`;
    }
}
