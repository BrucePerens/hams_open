/** @odoo-module **/

// The member-site editor's paste cleaner (RichEditor.cleanNode) on real DOM from the HTML parser, the way onPaste feeds it.
import { describe, expect, test } from "@odoo/hoot";
import { RichEditor } from "@user_websites/js/rich_editor";

const clean = (html) => {
    const editor = Object.create(RichEditor.prototype);
    return editor.cleanNode(new DOMParser().parseFromString(html, "text/html").body);
};

describe("rich_editor", () => {
    describe.current.tags("user_websites_rich_editor");

    test("pasted content keeps structure and loses scripts, classes and a link that is not http, mailto or a path", () => {
        // Tests [@ANCHOR: user_websites:rich_editor_clean_node]
        expect(clean('<h2 class="x">T</h2><p><b>b</b> <a href="https://e.org/?a=1">ok</a> <a href="javascript:alert(1)">bad</a><script>alert(1)</script></p>'))
            .toBe('<h2>T</h2><p><b>b</b> <a href="https://e.org/?a=1">ok</a> bad</p>');
    });

    test("an inline svg or math element adds no text to the page", () => {
        // Tests [@ANCHOR: user_websites:rich_editor_clean_node]
        // The parser keeps the tag names inside <svg> and <math> in lower case; the title and style text of an icon used to be pasted as page text.
        expect(clean("<p>Hi <svg><title>Icon</title><style>.a{fill:red}</style></svg>there</p>")).toBe("<p>Hi there</p>");
        expect(clean("<p>x<math><mi>y</mi></math>z</p>")).toBe("<p>xz</p>");
    });
});
