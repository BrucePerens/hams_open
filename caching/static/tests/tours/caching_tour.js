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
                // Tests [@ANCHOR: caching_sw_fetch_interceptor]
                // Verified by [@ANCHOR: test_caching_service_worker_tour]
                console.log('Tour started');
                if ('serviceWorker' in navigator) {
                    navigator.serviceWorker.getRegistrations().then(registrations => {
                        if (registrations.length > 0) {
                            console.log('Service Worker found');
                            document.body.classList.add('sw-registered');
                        } else {
                            throw new Error('No Service Worker found. Registration may have failed or was not initiated.');
                        }
                    });
                } else {
                    throw new Error('Service Worker is not supported by this browser environment.');
                }
            },
        },
        {
            content: "Wait for SW status to be updated",
            trigger: "body.sw-registered",
        }
    ],
});
