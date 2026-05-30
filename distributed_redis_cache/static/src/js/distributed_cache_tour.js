/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("distributed_cache_admin_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_main_navbar',
            content: "Wait for navbar",
        },
        {
            trigger: '.o_navbar_apps_menu button',
            content: "Open Apps Menu",
            run: "click",
        },
        {
            trigger: '.o_navbar_apps_menu.show',
            content: "Wait for Apps Menu to be visible",
            run: function() {}
        },
        {
            trigger: '[data-menu-xmlid="distributed_redis_cache.menu_distributed_cache_root"]',
            content: "Open Distributed Cache Manager",
            run: "click",
        },
        {
            trigger: 'button[name="check_redis_status"]',
            content: "Check Redis Status",
            run: "click",
        },
        {
            trigger: '.o_notification',
            content: "Wait for status notification to appear",
            run: function() {}
        },
        {
            trigger: '.o_field_widget[name="model_id"] input',
            content: "Input model name using native simulator",
            run: "edit User",
        },
        {
            trigger: 'body',
            content: "Wait for autocomplete dropdown and click the item",
            run: function () {
                return new Promise((resolve, reject) => {
                    let interval;
                    const timeoutId = setTimeout(() => {
                        clearInterval(interval);
                        reject(new Error("Model 'User' never appeared in dropdown."));
                    }, 10000);

                    interval = setInterval(() => {
                        const items = document.querySelectorAll('.dropdown-item, .o-autocomplete--dropdown-item');
                        for (const item of items) {
                            if (item.textContent.includes('User')) {
                                clearInterval(interval);
                                clearTimeout(timeoutId);
                                item.click();
                                resolve();
                                return;
                            }
                        }
                    }, 250);
                });
            }
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        },
        {
            trigger: 'button[name="action_invalidate_model_cache"]',
            content: "Invalidate the cache",
            run: "click",
        },
        {
            trigger: 'body',
            content: "Wait for RPC resolution and optional toast",
            run: function () {
                return new Promise((resolve) => {
                    let interval;

                    // If Redis is offline in the Jules VM, the backend safely returns False (no toast).
                    // We wait 3 seconds to ensure the fast RPC has safely resolved before teardown
                    // to prevent "dirty form" teardown crashes.
                    const timeoutId = setTimeout(() => {
                        clearInterval(interval);
                        resolve();
                    }, 3000);

                    interval = setInterval(() => {
                        const toast = document.querySelector('.toast-body, .o_notification');
                        const errorDialog = document.querySelector('.modal-body.text-danger, .o_notification_manager .text-danger');

                        if (toast || errorDialog) {
                            clearInterval(interval);
                            clearTimeout(timeoutId);
                            resolve();
                        }
                    }, 250);
                });
            }
        }
    ]
});
