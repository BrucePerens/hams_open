/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

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
            trigger: '.o_notification_manager',
            content: "Wait for status notification",
        },
        {
            trigger: '.o_field_widget[name="model_id"] input',
            content: "Input model name manually",
            run: function () {
                const el = document.querySelector('.o_field_widget[name="model_id"] input');
                el.value = 'User';
                el.dispatchEvent(new Event('input', { bubbles: true }));
                el.dispatchEvent(new Event('change', { bubbles: true }));
            }
        },
        {
            trigger: '.dropdown-item, .o-autocomplete--dropdown-item',
            content: "Select the model from autocomplete",
            run: function () {
                const items = document.querySelectorAll('.dropdown-item, .o-autocomplete--dropdown-item');
                for (const item of items) {
                    if (item.textContent.includes('User')) {
                        item.click();
                        break;
                    }
                }
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
            content: "Wait for the success toast to render",
            run: function () {
                return new Promise((resolve) => {
                    const interval = setInterval(() => {
                        const toast = document.querySelector('.toast-body') || document.querySelector('.o_notification_manager');
                        if (toast && toast.textContent.includes('Success')) {
                            clearInterval(interval);
                            resolve();
                        }
                    }, 250);
                });
            }
        }
    ]
});
