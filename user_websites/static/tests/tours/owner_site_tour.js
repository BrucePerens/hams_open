/** @odoo-module **/
import { registry } from "@web/core/registry";

// [@ANCHOR: test_tour_owner_site]
// Tests [@ANCHOR: user_websites:UX_OWNER_TOOLBAR]
// Tests [@ANCHOR: user_websites:UX_RICH_EDITOR]
// Tests [@ANCHOR: user_websites:rich_editor_apply_link]
// Tests [@ANCHOR: user_websites:rich_editor_clean_node]
// Tests [@ANCHOR: user_websites:rich_editor_close_link_bar]
// Tests [@ANCHOR: user_websites:rich_editor_image_picked]
// Tests [@ANCHOR: user_websites:rich_editor_link_bar_click]
// Tests [@ANCHOR: user_websites:rich_editor_link_keydown]
// Tests [@ANCHOR: user_websites:rich_editor_open_link_bar]
// Tests [@ANCHOR: user_websites:rich_editor_paste]
// Tests [@ANCHOR: user_websites:rich_editor_remember_range]
// Tests [@ANCHOR: user_websites:rich_editor_restore_range]
// Tests [@ANCHOR: user_websites:rich_editor_say]
// Tests [@ANCHOR: user_websites:rich_editor_start]
// Tests [@ANCHOR: user_websites:rich_editor_sync_field]
// Tests [@ANCHOR: user_websites:rich_editor_toolbar_click]
// Tests [@ANCHOR: user_websites:rich_editor_toolbar_state]
// Tests [@ANCHOR: user_websites:rich_editor_wrap_code]
// The owner opens their own site, uses the owner toolbar to start a blog post, writes it in the
// WYSIWYG editor (no markup), publishes, and sees it on the site.
registry.category("web_tour.tours").add("owner_site_tour", {
    url: "/touruser",
    steps: () => [
        {
            content: "The owner toolbar is on the owner's own site",
            trigger: ".uw-owner-toolbar",
        },
        {
            content: "Start a new blog post from the toolbar",
            trigger: ".uw-owner-toolbar form[action$='/create_blog_post'] button",
            run: "click",
            expectUnloadPage: true,
        },
        {
            content: "The editor is a rich-text area with a toolbar",
            trigger: ".uw-rich-editor [data-uw-area][contenteditable='true']",
        },
        {
            content: "Give the post a title",
            trigger: "#name",
            run: "edit Tour Post Title",
        },
        {
            content: "Write the body in the editor",
            trigger: ".uw-rich-editor [data-uw-area]",
            run() {
                const area = this.anchor;
                area.focus();
                document.execCommand("insertText", false, "Written without markup");
                const range = document.createRange();
                range.selectNodeContents(area);
                const selection = window.getSelection();
                selection.removeAllRanges();
                selection.addRange(range);
            },
        },
        {
            content: "Make it bold with the toolbar",
            trigger: ".uw-rich-editor button[data-uw-cmd='bold']",
            run: "click",
        },
        {
            content: "Publish it",
            trigger: "#is_published",
            run: "check",
        },
        {
            content: "Save",
            trigger: "form#blog_post_edit_form button[type='submit']",
            run: "click",
            expectUnloadPage: true,
        },
        {
            content: "The post is live on the site, formatted",
            trigger: "#wrapwrap b",
        },
    ],
});
