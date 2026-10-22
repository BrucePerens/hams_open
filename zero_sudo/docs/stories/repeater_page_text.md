<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Repeater's Callsign Is the One the Page Ties to the Repeater

As a **Club Webmaster**, I title my page "K&Oslash;HNY/R" and say "EchoLink node 469520 operated by KC&Oslash;REO", and I want hams.com to list the
repeater as K0HNY and not as the node operator's callsign or with a "/R" the FCC never licensed. As an **Administrator**, I want to
know when a page is old.

## The Process

1. A trailing repeater designator (/R, -R, /RPT, -RPT, /REP, " RPT") on a callsign-shaped token is removed from the repeater's callsign; the
   page's own text is kept in the notes (`callsign field as extracted: K&Oslash;HNY/R`). /P, /M, /QRP and country prefixes such as W1AW/VE3 are never touched
   `[@ANCHOR: zero_sudo:repeater_page_text_strip_repeater_designator]`.
2. A callsign the page names as the operator, owner or sponsor of an EchoLink, AllStar or IRLP node ("node 469520 operated by KC0REO") is found
   `[@ANCHOR: zero_sudo:repeater_page_text_node_operator_callsigns]`. When the page names another callsign
   `[@ANCHOR: zero_sudo:repeater_page_text_callsigns_in_text]`, the operator's is not accepted as the repeater's callsign.
3. A page that states its own date behind a label ("Last updated 14-Jan-2021", "Revised ...", "as of ...") `[@ANCHOR: zero_sudo:repeater_page_text_page_date]`,
   more than two years old, gets the note "page dated 14-Jan-2021, may be out of date" `[@ANCHOR: zero_sudo:repeater_page_text_stale_page_note]`. It is a note, never a rejection.

## Verification

`zero_sudo/tests/test_repeater_page_text.py` (`[@ANCHOR: zero_sudo:test_repeater_page_text_designator]`, `[@ANCHOR: zero_sudo:test_repeater_page_text_node_operators]`,
`[@ANCHOR: zero_sudo:test_repeater_page_text_stale]`) is built from that page's text.
