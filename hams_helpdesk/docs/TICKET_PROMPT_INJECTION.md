# Ticket prompt injection: policy for operators

Tickets, mail to the support aliases, and report forms are text written by strangers. Anything an AI,
an MCP tool or an agent session reads from them is untrusted data. An attacker who can hide an
instruction in a ticket can try to steer whatever reads it later. This policy is part of the shipped
code, so it protects every operator of the open-source modules, not only hams.com.

## What the code does

1. **One filter, one choke point.** `hams_helpdesk/models/untrusted_text.py` is a pure-Python filter.
   `hams_helpdesk.ticket` runs every untrusted field (subject, description, callsign, contact fields,
   and any field a module adds through `_untrusted_field_specs`) through it in `create()` and `write()`,
   and every message body, subject and sender name in `message_post()`. A portal form, inbound mail,
   the JSON-2 API, the GitHub webhook, and the report flows of other modules all end in those calls, so a
   new path cannot forget the filter. Nothing a caller sends can set the suspicion fields.
2. **Structural defence first.** HTML goes through a strict allow-list sanitizer: only the text of
   allow-listed tags survives; all `style`, `class`, `hidden` and event attributes are dropped; scripts,
   styles, templates, SVG, MathML, comments, CDATA, forms, images and data URIs are removed; content a
   browser would not show (display none, visibility hidden, opacity 0, tiny fonts, text colour equal or
   close to the background, off-screen positioning, zero-size overflow-hidden boxes, closed details,
   `aria-hidden`) is dropped. Its text is counted and a short escaped excerpt is recorded.
3. **Unicode.** Format, control, tag, variation-selector, bidi, filler, private-use, surrogate and
   unassigned characters are removed. Tag characters (U+E0000 to U+E007F, "ASCII smuggling") are decoded
   into a finding. Runs of combining marks are cut. Mixed-script words (Latin plus Cyrillic or Greek) are
   flagged, and homoglyphs and compatibility forms are folded for scanning only.
4. **Scanning, never rewriting.** Base64, hex, percent-encoding, entities, quoted-printable, ROT13,
   reversed and leetspeak variants are decoded for scanning. The decoded text never reaches a reader.
   Phrases that look like fake system messages, chat-template tokens, delimiter closers, tool-call JSON,
   "ignore previous instructions" (also in several languages), conceal requests and exfiltration requests
   raise the score. This is a signal, not the main defence.
5. **Findings are recorded.** Each ticket has `suspicion_score`, `suspicious` and a count of removed
   characters. Each event adds a `hams_helpdesk.ticket.suspicion` row (staff only): the findings (vector,
   location, count, escaped excerpt) and the text exactly as received, kept as evidence. The staff form
   shows a banner and a Filter Findings page. A ticket at or over the threshold is hidden from the
   external-AI triage account by an `ir.rule`, so it is never auto-triaged or noted by an AI.
6. **The safe view.** `ticket.safe_view()` and `mcp_safe_read()` return plain visible text, filtered
   again at read time (tickets stored before the filter are covered), wrapped in an untrusted-data block
   with an unpredictable per-call delimiter and a fixed statement that nothing inside is an instruction.
   A suspicious ticket is returned with no text. The triage MCP server reads tickets only this way and
   fails closed if the safe view is unavailable.
7. **Output filter.** `filter_ai_output()` strips markdown images, HTML, links and bare URLs (unless a
   host is explicitly allowed) and invisible characters from anything a model writes back, so a rendered
   image URL cannot leak data. Notes posted by the triage MCP pass through it.

## Rules for code that reads tickets

* Read a ticket through `safe_view()` or `mcp_safe_read()`. Never hand a raw `description`, raw mail
  HTML, raw headers or attachment bytes to a model.
* An AI that reads tickets gets no tool that writes, sends, fetches URLs or reads other tickets, and a
  human approves every action.
* Do not fetch a URL that appears in a ticket. Pasted logs and external text are untrusted and go
  through the same filter.
* A module that adds a free-text field to a ticket (or creates tickets from outside text) extends
  `_untrusted_field_specs()`; the regression corpus in `hams_helpdesk/tests/test_untrusted_text.py`
  gets a case for every new trick.

## Known limits

The scoring phrases are heuristics and can be bypassed; the structural controls (safe view, untrusted
block, least privilege, output filter, human approval) are what protect. Attachment text is not given to
an AI. A feature that needs it must extract visible text only, strip metadata, and refuse archives.
