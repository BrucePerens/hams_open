# Odoo 19 UI Tour Development Guide

JavaScript UI Tours in Odoo are inherently brittle. This document centralizes all architectural mandates, workarounds, and syntax rules required to write stable, non-flaky tours that pass the CI/CD pipeline.

## 1. Tour Registration & Setup
* **Category:** Tours MUST be registered under `web_tour.tours` (e.g., `registry.category("web_tour.tours").add(...)`). Do not use legacy `tours` or `web_tours`.
* **Starting URL:** Always use explicit query parameters and include the debug flag (e.g., `url: "/odoo?debug=1&action=..."`). Do not use hash-based routing (`/web#...`).
* **Authentication:** Headless browser tests start unauthenticated. Python tests executing authenticated flows must explicitly pass the `login` keyword to `start_tour()`.

## 2. DOM Targeting & Selectors
* **Native-First:** Prioritize `name` attributes (`button[name="action_install"]`, `input[name="login"]`) over structural layout classes.
* **Avoid `:contains`:** The pseudo-selector `:contains(...)` crashes Odoo 19's native `document.querySelectorAll()`. Target elements by name, ID, or structural class instead.
* **Invisible Elements (The 0x0 Bypass):** The tour framework ignores elements calculating to 0x0 pixels. If you must target a hidden dropzone, a `display: contents` wrapper, or a readonly span to verify a DOM state change, you MUST append the `:not(:visible)` pseudo-selector to bypass the strict bounding-box visibility check.
* **Legacy Tags:** Native `<select>` and `<option>` tags are deprecated in Odoo 19 form views. Use `.o_select_menu` and `.o_select_menu_item`.
* **Autocomplete Dropdowns:** jQuery autocomplete (`.ui-menu-item`) is removed. Target `.o-autocomplete--dropdown-item` or `.dropdown-item`.

## 3. Input Simulation & Blurring
* **Action `edit` vs `text`:** Use `run: 'edit <value>'`. The action `text` is invalid in Odoo 19 and will crash the runner.
* **The "Dirty Form" Race Condition:** Odoo form buttons (`type="object"`) save the form asynchronously. If text is entered (`edit`) and an action button or save button is clicked immediately, the DOM `blur` event hasn't fired, resulting in data loss.
* **Neutral Click-Away:** You MUST inject a neutral "click away" step (e.g., `trigger: '.o_form_sheet'`, `run: 'click'`) after text entry and before clicking an action or save button to commit the DOM state.

## 4. Safe Saves & RPC Resolution
* **Safe Save Macro:** Never manually click `.o_form_button_save` and end the tour. You MUST append `.concat(TourUtils.safeSave())` to your steps array. (Note: Do NOT use the ES6 spread operator `...TourUtils` as it crashes the `rjsmin` asset minifier).
* **RPC Resolution (True vs Action):** When clicking a backend action button:
    * If the backend returns `True`, Odoo silently reloads the form. It DOES NOT spawn a notification. You MUST wait for a verifiable DOM state change.
    * **False Positive CSS Traps:** Do not rely exclusively on CSS classes like `:not(.o_field_empty)` to verify form reloads. In Odoo 19, readonly fields may lack this class initially, causing instant false positive successes that crash the teardown process.
    * **Promise-Based Polling:** To bulletproof RPC resolution checks, use a custom `Promise` loop with `setInterval` that physically reads the `textContent` of the target DOM node (e.g., checking `/\d/.test(field.textContent)`) to guarantee the backend data has rendered.
    * If the backend explicitly returns a notification/action, wait for it safely using `.o_notification` or `TourUtils.waitForAbsence()`.

## 5. Modals & Dialogs
* **Native Dialogs:** `window.alert` and `window.confirm` freeze headless Chrome. Bypass them by injecting a window override on the `body` in a preceding step.
* **Modal Targeting (Strict Rules):** Structural wrappers like `.modal-content` or `.modal-dialog` MUST ONLY be used for passive DOM polling (`run: function() {}`) to wait for a modal to mount and render. Attempting to click them directly will trip linter fragility checks.
* **Modal Click-Away:** When forcing a DOM blur inside a modal to commit text input, you MUST click a neutral safe zone inside the modal, such as `.modal-body`.

## 6. Page Unloads
* **Form Submissions:** When a tour clicks a `type="submit"` button or triggers a hard browser navigation (bypassing the SPA router), you MUST explicitly declare `expectUnloadPage: true` on that step. Failing to do so causes the tour runner to crash on the browser's `beforeUnload` event.
* You MUST use Odoo's native helper `run: 'click'` on unload steps. Custom closures (`run: () => {...}`) break the unload event binding.
* **Post-Unload Waiting:** Immediately after an `expectUnloadPage` step, provide a neutral wait step (e.g., `trigger: 'body', run: function() {}`) to allow the new page DOM to hydrate before asserting success.
