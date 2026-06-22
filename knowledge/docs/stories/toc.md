# Story: Dynamic Table of Contents
[@ANCHOR: story_manual_toc]

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
- Verification: `[@ANCHOR: test_tour_manual_toc]`
