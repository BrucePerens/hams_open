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

4. The page's structure decides whose a repeater is, in code, whatever the model said. Headings such as 'Other Local Repeaters', 'Local and Regional Repeaters',
   'Useful ... Repeaters' or 'KERN Linked Repeater System' set the repeaters under them apart `[@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]`; a callsign taken from another
   block is removed `[@ANCHOR: zero_sudo:repeater_page_text_callsign_adjacent]`; the club's own callsign (the title bar, the footer, with an APRS suffix or /R ignored)
   `[@ANCHOR: zero_sudo:repeater_page_text_header_callsigns]` `[@ANCHOR: zero_sudo:repeater_page_text_callsign_core]`, a club section, 'our' or 'operated by <the club>' are club evidence
   `[@ANCHOR: zero_sudo:repeater_page_text_row_ownership]`; a list of several licensees with none of that is 'unclear' `[@ANCHOR: zero_sudo:repeater_page_text_apply_page_checks]`.
5. A repeater whose frequency is not on the page was invented (`[@ANCHOR: zero_sudo:repeater_page_text_ground_item]`, `[@ANCHOR: zero_sudo:repeater_page_text_frequency_on_page]`,
   `[@ANCHOR: zero_sudo:repeater_page_text_number_on_page]`, `[@ANCHOR: zero_sudo:repeater_page_text_grounded_notes]`), and a page that is only an unedited default page has none to list
   `[@ANCHOR: zero_sudo:repeater_page_text_placeholder_page]`.

6. A "repeater" in orbit (the ISS cross-band repeater, an amateur satellite) is recognised from the item's own name, section, address, callsign (NA1SS, RS0ISS, ...) or an uplink/downlink description
   `[@ANCHOR: zero_sudo:repeater_page_text_orbiting_repeater]`; a terrestrial repeater that only mentions satellites, ISS inside a longer word, a club that makes ARISS contacts, or a club called OSCAR on 145.490 is not.

Helpers: `[@ANCHOR: zero_sudo:repeater_page_text_append_note]`, `[@ANCHOR: zero_sudo:repeater_page_text_clean]`, `[@ANCHOR: zero_sudo:repeater_page_text_frequencies]`, `[@ANCHOR: zero_sudo:repeater_page_text_heading_kind]`, `[@ANCHOR: zero_sudo:repeater_page_text_item_frequency]`, `[@ANCHOR: zero_sudo:repeater_page_text_month]`, `[@ANCHOR: zero_sudo:repeater_page_text_near]`, `[@ANCHOR: zero_sudo:repeater_page_text_occurrences]`, `[@ANCHOR: zero_sudo:repeater_page_text_own_span]`, `[@ANCHOR: zero_sudo:repeater_page_text_page_numbers]`, `[@ANCHOR: zero_sudo:repeater_page_text_structure]`, `[@ANCHOR: zero_sudo:repeater_page_text_title]`, `[@ANCHOR: zero_sudo:repeater_page_text_title_words]`, `[@ANCHOR: zero_sudo:repeater_page_text_to_date]`.

## Verification

`zero_sudo/tests/test_repeater_page_text.py` (`[@ANCHOR: zero_sudo:test_repeater_page_text_designator]`, `[@ANCHOR: zero_sudo:test_repeater_page_text_node_operators]`,
`[@ANCHOR: zero_sudo:test_repeater_page_text_stale]`) is built from that page's text.
