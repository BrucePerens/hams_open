/** Copyright © HAMS project. AGPL-3.0-or-later. **/
/** @odoo-module **/

import { Component, useState, xml } from "@odoo/owl";
import { registry } from "@web/core/registry";

export class SWToast extends Component {
    static template = xml`
        <div t-if="state.show" class="position-fixed bottom-0 end-0 p-3" style="z-index: 9999;">
            <div class="toast show text-bg-primary" role="alert" aria-live="assertive" aria-atomic="true">
                <div class="toast-header">
                    <strong class="me-auto">Update Available</strong>
                    <button type="button" class="btn-close" aria-label="Close" t-on-click="close"></button>
                </div>
                <div class="toast-body">
                    A new version of this app is available.
                    <button class="btn btn-light btn-sm ms-2" t-on-click="reload">Reload</button>
                </div>
            </div>
        </div>
    `;

    setup() {
        this.state = useState({ show: false });

        if ('serviceWorker' in navigator) {
            navigator.serviceWorker.addEventListener('message', (event) => {
                if (event.data && event.data.type === 'NEW_VERSION_INSTALLED') {
                    this._showOnceModalClear();
                }
            });
        }
    }

    // [@ANCHOR: caching:toast_deferred_while_modal_open]
    // Bug fix (2026-09-24, found live during usability-audit testing): this toast is a real
    // website.published-multi-mixin `main_components` component, so it renders on every page --
    // including the very first page a brand-new visitor ever loads, at the same moment Odoo's own
    // cookie-consent bar is up. Both are fixed-position and both anchor to the bottom of the
    // viewport; this toast's own z-index (9999, well above Bootstrap's default
    // `$zindex-modal: 1055`) then sits directly on top of the consent bar's buttons and eats every
    // click meant for them (confirmed live: a real Playwright click on "Only essentials" failed
    // outright, `<div class="toast-body">...intercepts pointer events`). A fixed reposition
    // (top-right instead of bottom-right, say) would only trade one hardcoded, breakpoint-fragile
    // collision for another -- this defers the toast's own reveal instead, until no Bootstrap
    // modal is currently open, which is correct regardless of viewport size and generalizes to any
    // other modal this site might ever show at the same moment, not just this one consent bar.
    _showOnceModalClear() {
        if (document.querySelector('.modal.show')) {
            document.addEventListener(
                'hidden.bs.modal',
                () => this._showOnceModalClear(),
                { once: true }
            );
            return;
        }
        this.state.show = true;
    }

    close() {
        this.state.show = false;
    }

    reload() {
        document.location.reload();
    }
}

registry.category("main_components").add("caching.SWToast", {
    Component: SWToast,
});
