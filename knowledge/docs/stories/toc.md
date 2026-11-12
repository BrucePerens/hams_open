# Story: Dynamic Table of Contents

## Summary [@ANCHOR: story_manual_toc]

This story is the table of contents on a long article. A small browser widget, `ManualTOC`, runs when the article page loads, collects the `h2` and `h3` headings of the article body, gives each an id if it has none, and fills the sticky container on the side with a list of links to them. An article without such headings gets no table of contents, and no server code is involved.

This story describes how a Table of Contents (TOC) is automatically generated for articles.

## Scenario
A user views a long article with multiple sections.

## Process
1. The article page is loaded in the browser.
2. The `ManualTOC` JavaScript widget `[@ANCHOR: manual_toc_logic]` initializes.
3. The widget scans the article body for `<h2>` and `<h3>` headings.
4. It generates a structured list of links based on these headings.
5. The list is injected into the sticky TOC container on the side of the page.

## Technical Details
- JS Widget: `publicWidget.registry.ManualTOC`
- Logic: Vanilla JS DOM manipulation.
### Verification [@ANCHOR: test_tour_manual_toc]

`tests/test_ui_tours.py` `test_01_manual_toc_tour` creates a published article whose body has an `<h2>` and an `<h3>` and runs `manual_toc_tour` (`static/tests/tours/manual_toc_tour.js`), which waits for the `#manual_toc_container` to contain a list and for a generated link whose target begins `#toc-heading-`. It proves the widget ran in a real page and needs a browser.
