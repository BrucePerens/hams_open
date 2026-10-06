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

8. **Inbound mail.** `mail.thread.message_parse()` (`models/mail_thread_untrusted.py`) inspects every
   parsed mail before anything reads it, because Odoo's own parse keeps only one part of a
   multipart/alternative and flattens headers. A header allow-list scans From, Sender, Reply-To, To, Cc,
   Subject, Message-ID and the threading headers with the same text filter (display text is judged;
   routing values such as Message-ID are flagged and never rewritten, so threading keeps working), scans
   free-text headers outside the list (Comments, Keywords, Organization...), and flags a second copy of a
   singleton header (two Subject lines). The visible text of the text/plain and the text/html part of
   each alternative are compared and a mismatch is flagged. The findings are recorded on the ticket by
   its `message_post()` (or by pager_duty's mail route, which makes the ticket itself). A failure of the
   inspection marks the mail suspicious; it never drops or clears it.
9. **Attachments are never read by an AI.** File names are cleaned (hidden, bidi and control characters
   removed, one line) and counted; archives and nested messages are flagged. Every AI read states that
   attachments were not read and how many exist (`attachments_notice`). A feature that needs attachment
   text must extract visible text only, strip metadata and refuse archives; none does today.
10. **Every AI reader goes through the safe view.** Tickets (`mcp_safe_read`), customer follow-up messages
    (`mcp_customer_followups`: filtered, cut to length, then wrapped in the same random-delimiter block;
    withheld when suspicious), pager_duty incidents (`mcp_list_incidents`, `mcp_get_incident_detail`: name,
    source, description and every chatter message are filtered and returned only inside the block; the
    AI's own note is output-filtered), and the scraped event text that the event enrichment daemon and its
    MCP server give a model. A daemon fails closed: no safe view means the text is withheld, never raw.
11. **A rule keeps it that way.** `ai_reader_rule.py` is a static AST rule, run by
    `tests/test_ai_reader_sites.py` here (and over hams_com by its `daemons/test_ticket_ai_reads.py`). It
    fails when an `mcp_*` method on a ticket-like model reads free text without the filter, or when a
    daemon that talks to a model reads a ticket-like model's text fields without the safe view. Silence a
    false positive with `# ticket-ai-ignore: <reason>`; never to skip the filter.

## Rules for code that reads tickets

* Read a ticket through `safe_view()` or `mcp_safe_read()`. Never hand a raw `description`, raw mail
  HTML, raw headers or attachment bytes to a model.
* An AI that reads tickets gets no tool that writes, sends, fetches URLs or reads other tickets, and a
  human approves every action.
* Never give an AI attachment content, and make the read say so (`attachments_notice`).
* Do not fetch a URL that appears in a ticket. Pasted logs and external text are untrusted and go
  through the same filter.
* A module that adds a free-text field to a ticket (or creates tickets from outside text) extends
  `_untrusted_field_specs()`; the regression corpus in `hams_helpdesk/tests/test_untrusted_text.py`
  gets a case for every new trick.

## Known limits

The scoring phrases are heuristics and can be bypassed; the structural controls (safe view, untrusted
block, least privilege, output filter, human approval) are what protect. Attachment text is not given to
an AI. A feature that needs it must extract visible text only, strip metadata, and refuse archives. The
static rule is an AST heuristic, not dataflow analysis: it catches the shapes in use today (an `mcp_*`
method, a daemon read through `execute`), and a new kind of reader needs the rule extended.
