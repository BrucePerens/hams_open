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
            content: "Click on an article link in the sidebar or main content",
            trigger: 'a.o_manual_article_link',
            run: 'click'
        },
        {
            content: "Verify article loaded properly",
            trigger: '.o_manual_content, .article-content',
            run: function () {}
        }
    ]
});
