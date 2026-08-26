/** Copyright © HAMS project. AGPL-3.0-or-later. **/
/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("caching_service_worker_check", {
    url: "/?debug=1",
    steps: () => [
        {
            content: "Wait for page to load",
            trigger: "body",
        },
        {
            content: "Check if Service Worker is supported and registered",
            trigger: "body",
            run: function () {
                // Tests [@ANCHOR: COMM_caching_sw_fetch_interceptor]
                if ('serviceWorker' in navigator) {
                    navigator.serviceWorker.ready.then(() => {
                        document.body.classList.add('sw-registered');
                        return;
                    }).catch((err) => {
                        console.error('[caching_tour] serviceWorker.ready rejected:', err);
                    });
                } else {
                    // [!] DIAGNOSTIC FOR AI: Service Worker is not supported.
                    // Check browser environment and HTTPS/localhost requirements.
                    throw new Error('Service Worker is not supported by this browser environment.');
                }
            },
        },
        {
            content: "Wait for SW status to be updated",
            trigger: 'body.sw-registered',
            run: function() {}
        },
    ],
});
