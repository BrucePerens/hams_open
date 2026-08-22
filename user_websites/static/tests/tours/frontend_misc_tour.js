/** @odoo-module **/
import { registry } from "@web/core/registry";


// Tests [@ANCHOR: test_tour_frontend_misc]
registry.category("web_tour.tours").add("frontend_misc_tour", {
    url: "/user-websites/documentation",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            // /user-websites/documentation (controllers/main.py's
            // `documentation()`) redirects here once the manifest-declared
            // knowledge_docs entry has been bootstrapped by
            // zero_sudo/models/ir_module_module.py's _bootstrap_knowledge_docs
            // (article.website_url is always non-empty once the article has
            // an id -- see knowledge_article.py's _compute_website_url --
            // so the QWeb documentation_page fallback in this module's own
            // user_websites_templates.xml is unreachable once the article
            // exists, which it does in any fully-installed environment).
            trigger: '#main-content .o_manual_body',
            content: 'Wait for the real Knowledge article body to render at /manual/<id>-...',
            run: function () {},
        },
        {
            trigger: 'h1.display-4',
            content: "Verify Documentation Page renders correctly (real article title, not the QWeb fallback stub)",
            run: function () {
                if (!document.querySelector('h1.display-4').textContent.includes('User Websites Documentation')) {
                    throw new Error(
                        "Expected the real 'User Websites Documentation' knowledge article title, got: " +
                        document.querySelector('h1.display-4').textContent
                    );
                }
            },
        },
    ],
});
