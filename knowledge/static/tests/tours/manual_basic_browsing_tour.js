/** @odoo-module **/

import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add('manual_basic_browsing_tour', {
    url: '/manual',
    steps: () => [
        {
            content: "Check that the manual page loaded and wait for articles",
            trigger: '#manual_toc_container, .o_manual_article_link',
            run: function () {} // Just wait for it
        },

        {
            content: "Wait for portal assets to load to avoid race condition",
            trigger: 'body',
            run: function () {
                // Odoo's tour runner sometimes incorrectly flags invisible modals as blocking
                document.querySelectorAll('.modal').forEach(el => el.remove());
                document.querySelectorAll('.modal-backdrop').forEach(el => el.remove());
                document.body.classList.remove('modal-open');
                return new Promise(resolve => setTimeout(resolve, 1000));
            }
        },
        {
            content: "Click on an article link in the sidebar or main content",
            trigger: 'body',
            run: function () {
                document.querySelectorAll('.modal').forEach(el => el.remove());
                document.querySelectorAll('.modal-backdrop').forEach(el => el.remove());
                document.body.classList.remove('modal-open');
                
                const link = document.querySelector('aside.d-none.d-lg-block a.o_manual_article_link');
                if (link) {
                    link.click();
                }
            },
            expectUnloadPage: true
        },
        {
            content: "Verify article loaded properly",
            trigger: '.o_manual_body, .article-body',
            run: function () {}
        }
    ]
});
