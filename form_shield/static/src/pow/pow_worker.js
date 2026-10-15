/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * [@ANCHOR: form_shield:pow_worker_js]
 * Verified by [@ANCHOR: form_shield:test_pow_worker_runs_in_a_real_browser]
 *
 * Web Worker entry. Served as this plain static file (not from a blob:, not inside an asset
 * bundle) so a `worker-src 'self'` / `script-src 'self'` policy admits it. Receives
 * {challenge, formId, difficulty}; answers {counter} when solved, or {error}.
 */
importScripts("/form_shield/static/src/pow/pow_core.js");

self.onmessage = function (event) {
    const { challenge, formId, difficulty } = event.data;
    try {
        const counter = self.FormShieldPow.solve(challenge, formId, difficulty);
        self.postMessage({ counter });
    } catch (error) {
        self.postMessage({ error: String(error) });
    }
};
