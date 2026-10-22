/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

// Starts the browser JavaScript error reporter on this page. Kept apart from browser_error_reporter.js
// so the unit tests can import the reporter without it listening to the test runner's own errors.
// Loaded by web.assets_frontend (website and portal) and web.assets_backend (the back office) only.
import { install } from "@hams_helpdesk/js/browser_error_reporter";

install(window);
