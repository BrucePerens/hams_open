/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

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
                // Tests [@ANCHOR: caching_sw_fetch_interceptor]
                // Verified by [@ANCHOR: test_caching_service_worker_tour]
                if ('serviceWorker' in navigator) {
                    navigator.serviceWorker.ready.then(() => {
                        document.body.classList.add('sw-registered');
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
