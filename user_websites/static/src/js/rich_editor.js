/** @odoo-module **/

import { Interaction } from "@web/public/interaction";
import { registry } from "@web/core/registry";

// The member-site WYSIWYG editor: a contenteditable area with a toolbar. Members format the page
// itself and never see markup. The server is the authority on what is kept
// (user_websites/models/member_html.py); this file only produces sensible markup and cleans
// pasted content on the way in. No external library, no CDN.

const BLOCK_TAGS = new Set(["P", "H1", "H2", "H3", "H4", "UL", "OL", "LI", "BLOCKQUOTE", "PRE", "DIV", "BR", "HR", "TABLE", "THEAD", "TBODY", "TR", "TD", "TH", "FIGURE", "FIGCAPTION"]);
const INLINE_TAGS = new Set(["STRONG", "B", "EM", "I", "U", "S", "STRIKE", "SUB", "SUP", "CODE", "A", "IMG", "SPAN"]);
const DROP_TAGS = new Set(["SCRIPT", "STYLE", "IFRAME", "OBJECT", "EMBED", "FORM", "INPUT", "BUTTON", "SELECT", "TEXTAREA", "LINK", "META", "SVG", "MATH", "NOSCRIPT", "TEMPLATE", "VIDEO", "AUDIO"]);
const TOGGLE_STATE = {
    bold: "bold",
    italic: "italic",
    underline: "underline",
    insertUnorderedList: "insertUnorderedList",
    insertOrderedList: "insertOrderedList",
    justifyLeft: "justifyLeft",
    justifyCenter: "justifyCenter",
    justifyRight: "justifyRight",
};

export class RichEditor extends Interaction {
    static selector = ".uw-rich-editor";

    // [@ANCHOR: user_websites:rich_editor_start]
    // Verified by [@ANCHOR: test_tour_owner_site]
    start() {
        this.area = this.el.querySelector("[data-uw-area]");
        this.field = this.el.querySelector("[data-uw-field]");
        this.status = this.el.querySelector("[data-uw-status]");
        this.linkBar = this.el.querySelector(".uw-link-bar");
        this.linkInput = this.el.querySelector("[data-uw-link-input]");
        this.fileInput = this.el.querySelector("[data-uw-file]");
        this.savedRange = null;
        this.form = this.el.closest("form");
        this.syncField();
        this.addListener(this.el.querySelector("[data-uw-toolbar]"), "click", this.onToolbarClick.bind(this));
        this.addListener(this.el.querySelector("[data-uw-toolbar]"), "mousedown", (ev) => {
            // Keep the selection in the page while a toolbar button is pressed.
            if (ev.target.closest("button")) {
                ev.preventDefault();
            }
        });
        this.addListener(this.area, "paste", this.onPaste.bind(this));
        this.addListener(this.area, "input", this.syncField.bind(this));
        this.addListener(this.area, "keyup", this.refreshToolbarState.bind(this));
        this.addListener(this.area, "mouseup", this.refreshToolbarState.bind(this));
        this.addListener(this.linkBar, "click", this.onLinkBarClick.bind(this));
        this.addListener(this.linkInput, "keydown", this.onLinkKeydown.bind(this));
        this.addListener(this.fileInput, "change", this.onImagePicked.bind(this));
        if (this.form) {
            this.addListener(this.form, "submit", this.syncField.bind(this));
        }
    }

    // [@ANCHOR: user_websites:rich_editor_sync_field]
    // Verified by [@ANCHOR: test_tour_owner_site]
    syncField() {
        this.field.value = this.area.innerHTML;
    }

    // [@ANCHOR: user_websites:rich_editor_say]
    // Verified by [@ANCHOR: test_tour_owner_site]
    say(message) {
        this.status.textContent = message || "";
    }

    // [@ANCHOR: user_websites:rich_editor_toolbar_click]
    // Verified by [@ANCHOR: test_tour_owner_site]
    onToolbarClick(ev) {
        const button = ev.target.closest("button[data-uw-cmd]");
        if (!button || button.disabled) {
            return;
        }
        const cmd = button.dataset.uwCmd;
        this.area.focus();
        if (cmd === "link") {
            this.openLinkBar();
        } else if (cmd === "image") {
            this.rememberRange();
            this.fileInput.click();
        } else if (cmd === "code") {
            this.wrapSelectionInCode();
        } else if (cmd === "formatBlock") {
            document.execCommand("formatBlock", false, button.dataset.uwArg);
        } else {
            document.execCommand(cmd, false, null);
        }
        this.syncField();
        this.refreshToolbarState();
    }

    // [@ANCHOR: user_websites:rich_editor_toolbar_state]
    // Verified by [@ANCHOR: test_tour_owner_site]
    refreshToolbarState() {
        for (const button of this.el.querySelectorAll("button[data-uw-cmd]")) {
            const state = TOGGLE_STATE[button.dataset.uwCmd];
            if (state) {
                let on = false;
                try {
                    on = document.queryCommandState(state);
                } catch {
                    on = false;
                }
                button.setAttribute("aria-pressed", on ? "true" : "false");
                button.classList.toggle("active", on);
            }
        }
    }

    // [@ANCHOR: user_websites:rich_editor_remember_range]
    // Verified by [@ANCHOR: test_tour_owner_site]
    rememberRange() {
        const selection = window.getSelection();
        if (selection.rangeCount && this.area.contains(selection.anchorNode)) {
            this.savedRange = selection.getRangeAt(0).cloneRange();
        }
    }

    // [@ANCHOR: user_websites:rich_editor_restore_range]
    // Verified by [@ANCHOR: test_tour_owner_site]
    restoreRange() {
        this.area.focus();
        if (this.savedRange) {
            const selection = window.getSelection();
            selection.removeAllRanges();
            selection.addRange(this.savedRange);
        }
    }

    // [@ANCHOR: user_websites:rich_editor_wrap_code]
    // Verified by [@ANCHOR: test_tour_owner_site]
    wrapSelectionInCode() {
        const selection = window.getSelection();
        if (!selection.rangeCount || selection.isCollapsed) {
            return;
        }
        const text = selection.toString();
        const code = document.createElement("code");
        code.textContent = text;
        const range = selection.getRangeAt(0);
        range.deleteContents();
        range.insertNode(code);
    }

    // [@ANCHOR: user_websites:rich_editor_open_link_bar]
    // Verified by [@ANCHOR: test_tour_owner_site]
    openLinkBar() {
        this.rememberRange();
        this.linkBar.classList.remove("d-none");
        this.linkInput.value = "https://";
        this.linkInput.focus();
    }

    // [@ANCHOR: user_websites:rich_editor_close_link_bar]
    // Verified by [@ANCHOR: test_tour_owner_site]
    closeLinkBar() {
        this.linkBar.classList.add("d-none");
        this.restoreRange();
    }

    // [@ANCHOR: user_websites:rich_editor_link_bar_click]
    // Verified by [@ANCHOR: test_tour_owner_site]
    onLinkBarClick(ev) {
        if (ev.target.closest("[data-uw-link-apply]")) {
            this.applyLink();
        } else if (ev.target.closest("[data-uw-link-cancel]")) {
            this.closeLinkBar();
        }
    }

    // [@ANCHOR: user_websites:rich_editor_link_keydown]
    // Verified by [@ANCHOR: test_tour_owner_site]
    onLinkKeydown(ev) {
        if (ev.key === "Enter") {
            ev.preventDefault();
            this.applyLink();
        } else if (ev.key === "Escape") {
            ev.preventDefault();
            this.closeLinkBar();
        }
    }

    // [@ANCHOR: user_websites:rich_editor_apply_link]
    // Verified by [@ANCHOR: test_tour_owner_site]
    applyLink() {
        const url = this.linkInput.value.trim();
        this.closeLinkBar();
        if (!/^(https?:\/\/|mailto:|\/)/i.test(url)) {
            this.say("Links must start with http://, https://, mailto: or /.");
            return;
        }
        document.execCommand("createLink", false, url);
        this.syncField();
    }

    // [@ANCHOR: user_websites:rich_editor_image_picked]
    // Verified by [@ANCHOR: test_tour_owner_site]
    async onImagePicked() {
        const file = this.fileInput.files[0];
        this.fileInput.value = "";
        if (!file) {
            return;
        }
        if (file.size > 2 * 1024 * 1024) {
            this.say("Images can be at most 2 MB.");
            return;
        }
        const data = new FormData();
        data.append("ufile", file);
        data.append("json_reply", "1");
        data.append("csrf_token", this.el.dataset.csrf);
        this.say("Uploading image...");
        let reply;
        try {
            const response = await fetch(this.el.dataset.uploadUrl, { method: "POST", body: data, credentials: "same-origin" });
            reply = await response.json();
        } catch {
            this.say("The image could not be uploaded. Try again.");
            return;
        }
        if (!reply.ok) {
            this.say(reply.error || "The image could not be uploaded.");
            return;
        }
        this.restoreRange();
        const alt = (file.name || "image").replace(/\.[^.]+$/, "").replace(/["<>&]/g, "");
        document.execCommand("insertHTML", false, `<img src="${reply.url}" alt="${alt}"/>`);
        this.syncField();
        this.say("Image added. Describe it in the page text for readers who cannot see it.");
    }

    // [@ANCHOR: user_websites:rich_editor_paste]
    // Verified by [@ANCHOR: test_tour_owner_site]
    onPaste(ev) {
        const clipboard = ev.clipboardData;
        if (!clipboard) {
            return;
        }
        ev.preventDefault();
        const html = clipboard.getData("text/html");
        if (html) {
            const doc = new DOMParser().parseFromString(html, "text/html");
            document.execCommand("insertHTML", false, this.cleanNode(doc.body));
        } else {
            document.execCommand("insertText", false, clipboard.getData("text/plain"));
        }
        this.syncField();
    }

    // [@ANCHOR: user_websites:rich_editor_clean_node]
    // Verified by [@ANCHOR: test_tour_owner_site]
    cleanNode(node) {
        // Pasted content keeps structure (headings, lists, emphasis, links) and loses everything
        // else: styles, classes, fonts, scripts. The server repeats the job authoritatively.
        let out = "";
        for (const child of node.childNodes) {
            if (child.nodeType === Node.TEXT_NODE) {
                out += child.textContent.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
            } else if (child.nodeType === Node.ELEMENT_NODE && !DROP_TAGS.has(child.tagName)) {
                const inner = this.cleanNode(child);
                const tag = child.tagName.toLowerCase();
                if (child.tagName === "A") {
                    const href = child.getAttribute("href") || "";
                    out += /^(https?:\/\/|mailto:|\/)/i.test(href) ? `<a href="${href.replace(/"/g, "&quot;")}">${inner}</a>` : inner;
                } else if (child.tagName === "IMG" || child.tagName === "BR" || child.tagName === "HR") {
                    out += child.tagName === "IMG" ? "" : `<${tag}/>`;
                } else if (BLOCK_TAGS.has(child.tagName) || INLINE_TAGS.has(child.tagName)) {
                    out += child.tagName === "SPAN" || child.tagName === "DIV" ? inner : `<${tag}>${inner}</${tag}>`;
                } else {
                    out += inner;
                }
            }
        }
        return out;
    }
}

registry.category("public.interactions").add("user_websites.RichEditor", RichEditor);
