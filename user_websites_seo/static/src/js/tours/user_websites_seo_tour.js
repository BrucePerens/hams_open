/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Verified by [@ANCHOR: test_seo_widget_tour]
registry.category("web_tour.tours").add("user_websites_seo_tour", {
    url: "/odoo?debug=1&action=base.action_res_users",
    steps: () => [
        {
            content: "Remove facet if needed and find the specific test user row by polling",
            trigger: 'body',
            run: function () {
                return new Promise((resolve) => {
                    let clickedFacet = false;
                    let logged = false;
                    const interval = setInterval(() => {
                        if (!logged) {
                            console.log("BODY HTML: ", document.body.innerHTML);
                            logged = true;
                        }
                        if (!clickedFacet) {
                            const facet = document.querySelector('.o_facet_remove');
                            if (facet) {
                                facet.click();
                                clickedFacet = true;
                                return; // wait for reload
                            }
                        }
                        
                        const cells = document.querySelectorAll('.o_data_row .o_data_cell[name="name"], .o_kanban_record');
                        for (const cell of cells) {
                            if (cell.textContent.includes("SEO UI Test User")) {
                                clearInterval(interval);
                                cell.click();
                                resolve();
                                return;
                            }
                        }
                    }, 250);
                });
            }
        },
        {
            content: "Click the SEO Metadata notebook tab injected by our module",
            trigger: 'a[name="user_websites_seo_settings"]',
            run: 'click',
        },
        {
            content: "Input SEO Meta Title",
            trigger: '.o_field_widget[name="website_meta_title"] input',
            run: 'edit SEO Title',
        },
        {
            content: "Input SEO Meta Description",
            trigger: '.o_field_widget[name="website_meta_description"] textarea',
            run: 'edit SEO Description',
        },
        {
            content: "Input SEO Meta Keywords",
            trigger: '.o_field_widget[name="website_meta_keywords"] input',
            run: 'edit SEO, Keywords, Odoo',
        },
        {
            content: 'Click away to force DOM blur and commit text input',
            trigger: '.o_form_sheet',
            run: 'click',
        }
    ].concat(TourUtils.safeSave()).concat([
        {
            content: 'Go back to list via breadcrumb to close the form',
            trigger: '.o_control_panel .breadcrumb-item:not(.active):first, .o_control_panel .o_back_button',
            run: 'click',
        },
        {
            content: "Wait for list view to load",
            trigger: '.o_list_table',
            run: function() {}
        },
    ]),
});
