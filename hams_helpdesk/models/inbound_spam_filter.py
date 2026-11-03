# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Real, explainable spam/phishing heuristic for inbound mail, applied at
mail-ingestion time -- per Bruce's own decision recorded in
night_shift_questions/answered/inbound-spam-filter-location-and-signal-
e14a6f8b.md ("Filter at mail-ingestion time"), not as a post-hoc
hams_helpdesk.ticket stage/tag and not left to the ticket-triage AI pass.

Inbound mail reaches a helpdesk ticket by two separate routes, and both
call this function:

- info@/postmaster@ -> pager.incident -> pager_duty's Helpdesk Adapter
  (pager_duty/models/incident_ticket_adapter.py,
  action_generate_helpdesk_ticket()), which creates a "[PAGER] ..." ticket.
- admin@/support@ -> hams_helpdesk.ticket directly, through the mail
  aliases in hams_helpdesk/data/mail_alias_data.xml and
  hams_helpdesk.ticket.message_new(). Production tickets #2 (same
  QuickBooks "negative feedback" campaign as phishing ticket #13) and #43
  ("Pending Violation Reports") arrived this way and so bypassed the
  filter while it lived only in pager_duty.

It lives in hams_helpdesk (a pure-Python module, no ORM use) because
pager_duty depends on hams_helpdesk, not the other way round, so this is
the one place both callers can import it from.

Deliberately NOT a machine-learning classifier (per the to-do's own "start
simple and defensible" instruction) -- every signal below is a plain,
auditable rule with its own human-readable reason string, so a reviewer
(or a future session) can see exactly why a message was flagged, not just
an opaque score. The bulk/marketing signals added 2026-11-03 are the one place
a sum is used: each weak signal is its own rule with its own reason and weight,
and the message is flagged only when the weights reach BULK_SCORE_THRESHOLD, so
one innocent feature of a real message (a List-Id, an "unsubscribe" link in a
vendor reply) can never flag it alone. See night_shift_todo/high/inbound-mail-ticket-ingestion-
has-no-spam-phishing-filter-e3a8f612.md for the real, live spam/phishing
examples this was built from.

False positives are the named risk, not false negatives: this module only
ever classifies, it never deletes or drops anything -- the caller is
responsible for routing a flagged message to a visible quarantine stage
that a human can still review and recover from (see incident_ticket_
adapter.py's and helpdesk_ticket.py's message_new() "spam" stage handling).
"""
import logging
import re
from email.utils import parseaddr
from urllib.parse import urlsplit

_logger = logging.getLogger(__name__)

# Known commercial-spam / scare-lure subject-line patterns, taken directly
# from the real "new"-stage tickets found live 2026-10-01 (see the to-do
# file referenced above for the full table). Each pattern carries its own
# plain-English reason so a flagged ticket's note says exactly why.
SPAM_SUBJECT_PATTERNS = [
    (
        re.compile(r"domain\(s\)\s+expiring\s+soon", re.IGNORECASE),
        "domain-renewal solicitation spam",
    ),
    (
        re.compile(r"unsuccessful\s+payment\s+for", re.IGNORECASE),
        "fake billing/payment-failure lure",
    ),
    (
        re.compile(r"high-potential\s+rfqs?\b", re.IGNORECASE),
        "wholesale/RFQ lead-generation spam",
    ),
    (
        re.compile(
            r"turn\s+your\s+online\s+store\s+into\s+an?\s+(ios|android|app)",
            re.IGNORECASE,
        ),
        "app-conversion marketing spam",
    ),
    (
        re.compile(r"high-intent\s+buyer\s+leads", re.IGNORECASE),
        "buyer-leads marketing spam",
    ),
    (
        re.compile(r"non[\s-]disclosure\s+agreement", re.IGNORECASE),
        "fake NDA / e-signature phishing lure",
    ),
    (
        re.compile(r"secure\s+document", re.IGNORECASE),
        "fake secure-document phishing lure",
    ),
    (
        re.compile(r"pending\s+violation\s+reports?", re.IGNORECASE),
        "compliance/violation scare spam",
    ),
    (
        re.compile(r"negative\s+(feedback|review)", re.IGNORECASE),
        "negative-review scare phishing",
    ),
    # Added 2026-11-03 from production tickets 113, 115 and 122 (names only).
    (
        re.compile(r"\bsubscription\s+renews?\s+soon", re.IGNORECASE),
        "fake subscription-renewal lure",
    ),
    (
        re.compile(r"\bsecurity\s+alert\s+for\s+\S+@\S+", re.IGNORECASE),
        "fake account security-alert lure",
    ),
    (
        re.compile(r"\bbalance\s+confirmation\b.*\baudit\b", re.IGNORECASE),
        "fake audit balance-confirmation lure",
    ),
]

# Brands actually impersonated in the real phishing tickets found live
# (ticket #13: QuickBooks/Intuit via a click.sleadtrack.com redirect to a
# non-Intuit domain; ticket #15: a ShareFile-branded "Secure document" /
# NDA lure) plus a few other commonly-impersonated brands, so the same
# display/actual-domain-mismatch check generalizes past just those two.
# Each brand's real domain set is deliberately narrow (its own registrable
# domains only) so a mismatch is a genuine signal, not a guess.
KNOWN_BRAND_DOMAINS = {
    "quickbooks": ("intuit.com",),
    "intuit": ("intuit.com",),
    "sharefile": ("sharefile.com", "citrix.com"),
    "docusign": ("docusign.com", "docusign.net"),
    "paypal": ("paypal.com",),
    "microsoft": ("microsoft.com", "office.com", "live.com"),
    "dropbox": ("dropbox.com",),
    "fedex": ("fedex.com",),
    "usps": ("usps.com",),
    "dhl": ("dhl.com",),
}

_HREF_RE = re.compile(r"""href\s*=\s*['"]([^'"]+)['"]""", re.IGNORECASE)


def _registrable_domain(hostname):
    """Strips a leading "www." only -- deliberately not a full public-
    suffix-list lookup (that's real complexity this heuristic doesn't
    need): every KNOWN_BRAND_DOMAINS entry above is already the brand's
    own exact registrable domain, so a simple suffix match
    (``hostname == domain or hostname.endswith("." + domain)``) is enough
    to tell "intuit.com"/"www.intuit.com"/"accounts.intuit.com" apart from
    an unrelated domain like "rwiwanksiit.vu"."""
    if not hostname:
        return ""
    hostname = hostname.lower().strip()
    if hostname.startswith("www."):
        hostname = hostname[4:]
    return hostname


def _domain_matches_brand(hostname, real_domains):
    domain = _registrable_domain(hostname)
    if not domain:
        return False
    return any(domain == real or domain.endswith("." + real) for real in real_domains)


def _extract_link_domains(body_html):
    domains = set()
    for href in _HREF_RE.findall(body_html or ""):
        # urlsplit() raises ValueError for a malformed host such as "http://[x/" ("Invalid IPv6 URL").
        # A hostile message is exactly the input this module must tolerate, and it runs inside the
        # mail ingestion path, so one bad href must not stop the ticket from being created.
        try:
            hostname = urlsplit(href).hostname
        except ValueError:
            continue
        if hostname:
            domains.add(hostname.lower())
    return domains


def _brand_mentioned(brand, text):
    return re.search(r"\b" + re.escape(brand) + r"\b", text, re.IGNORECASE) is not None


def detect_inbound_spam_signals(subject, body_html):
    """Returns a list of plain-English reason strings, one per matched
    signal -- empty if nothing matched. Never raises on malformed input
    (a hostile message is exactly the input this has to tolerate):
    unparseable links just contribute no domains, not an exception.

    ``subject``/``body_html`` are two of the three pieces of data already
    available at the point action_generate_helpdesk_ticket() runs
    (incident.name, incident.description) -- nothing here depends on the
    ORM or on a live network fetch. The sender address (incident.source)
    is deliberately NOT a parameter -- see the link-only rationale in the
    brand-impersonation check below.
    """
    subject = subject or ""
    body_html = body_html or ""
    reasons = []

    # Named "regex" (not "pattern") deliberately: check_burn_list.py's own
    # N+1-in-a-loop AST rule flags any ".search()" call inside a loop as a
    # likely ORM recordset search unless the receiver's name is "re",
    # contains "regex", or ends in "_RE" -- a compiled re.Pattern's own
    # .search() otherwise reads as a false positive for that rule.
    for regex, reason in SPAM_SUBJECT_PATTERNS:
        if regex.search(subject):
            reasons.append("subject matches known spam pattern: %s" % reason)

    combined_text = "%s\n%s" % (subject, body_html)
    link_domains = _extract_link_domains(body_html)

    # Deliberately gated on "the message actually contains a link", and
    # deliberately NOT checking the sender's own address domain here: a
    # genuine email ABOUT a brand (a colleague sharing a real DocuSign
    # envelope, a customer asking "is this FedEx email legit?") routinely
    # comes from an ordinary third-party address that has nothing to do
    # with that brand's own domain -- only a mismatched LINK is the real
    # signal, confirmed directly in both real phishing tickets (#13's
    # click.sleadtrack.com redirect, #15's non-ShareFile sign-in link).
    # Checking the sender's domain too (an earlier version of this
    # function did) flagged ordinary legitimate mail as a false positive
    # whenever it merely named a brand from a non-brand address -- exactly
    # the risk the to-do calls out -- so it was dropped in favor of this
    # narrower, link-only check.
    if not link_domains:
        return reasons

    for brand, real_domains in KNOWN_BRAND_DOMAINS.items():
        if not _brand_mentioned(brand, combined_text):
            continue
        mismatched = {
            domain
            for domain in link_domains
            if not _domain_matches_brand(domain, real_domains)
        }
        if mismatched:
            reasons.append(
                "possible %s impersonation: message mentions \"%s\" but "
                "link domain(s) %s do not match %s's real domain(s) %s"
                % (
                    brand,
                    brand,
                    ", ".join(sorted(mismatched)),
                    brand,
                    ", ".join(real_domains),
                )
            )

    return reasons


# [@ANCHOR: hams_helpdesk:ses_verdicts]
# Verified by [@ANCHOR: test_ses_verdicts_flag_only_a_fail]
# Amazon SES scans mail when the receipt rule has ScanEnabled (hams.com's does) and writes the result into the
# stored message as X-SES-Spam-Verdict and X-SES-Virus-Verdict, each PASS, FAIL, GRAY or PROCESSING_FAILED.
# Only a FAIL is used: GRAY and PROCESSING_FAILED mean SES could not decide. The authentication results (DKIM, SPF,
# DMARC) are NOT strong signals: mailing lists and forwarders legitimately fail them. Since 2026-11-03 a DMARC, SPF
# or DKIM failure is a weak, weighted bulk signal (see bulk_mail_findings), never a reason by itself.
_SES_VERDICT_HEADERS = (
    ("spam", "X-SES-Spam-Verdict"),
    ("virus", "X-SES-Virus-Verdict"),
)


def ses_verdicts(message):
    """The SES scan verdicts of a parsed ``email.message.Message``, as {"spam": "FAIL", "virus": "PASS"}; a
    verdict header that is absent is absent from the dict. Values are upper-cased and cut to 32 characters
    (the sender controls any header that SES did not itself write)."""
    found = {}
    for key, header in _SES_VERDICT_HEADERS:
        value = message.get(header)
        if value:
            found[key] = str(value).strip().upper()[:32]
    return found


def ses_verdict_reasons(verdicts):
    """Plain-English reasons for the verdicts that say FAIL; empty for anything else."""
    reasons = []
    if (verdicts or {}).get("spam") == "FAIL":
        reasons.append("Amazon SES spam scan verdict: FAIL")
    if (verdicts or {}).get("virus") == "FAIL":
        reasons.append("Amazon SES virus scan verdict: FAIL")
    return reasons


# ----------------------------------------------------------------------------------------------------------------
# Bulk / marketing mail (2026-11-03). Production tickets 113-140 were HTML marketing newsletters that matched no
# subject pattern. Each signal below is its own plain rule with its own reason; the weights add up and the message
# is flagged only at BULK_SCORE_THRESHOLD, so no single weak signal can quarantine a message. The inputs are the
# facts mail_signals() reads from the raw MIME message in mail.thread.message_parse() (the sanitised ticket body
# has lost the images, the plain-text part and every header), plus the subject and body.
# ----------------------------------------------------------------------------------------------------------------

# The message is flagged when its bulk signals weigh at least this much. The largest single weight is 2, so three
# independent facts are always required. A vendor's real notice often carries two of them (an unsubscribe link and a
# branded table layout); it takes a third (no plain-text part, a tracking pixel, a clickbait subject) to flag it.
BULK_SCORE_THRESHOLD = 5

# Context key under which pager.incident.message_new() hands the adapter the reasons it already computed for this
# one message (a list; empty means "judged, clean"), so the message is judged once and the adapter does not re-judge it
# without the headers the adapter cannot see.
HAMS_INBOUND_SPAM_REASONS_CONTEXT_KEY = "hams_inbound_spam_reasons"

# The retroactive pass over tickets already stored (hams_com devbox_tools/reclassify_bulk_spam_tickets_hams_prod.py)
# has only the subject and the sanitised body: no headers, no plain-text part, no images. It scores one lower, and
# a person reads its dry run before anything changes.
STORED_TICKET_SCORE_THRESHOLD = 4

# Sender domains that are ours. Mail From one of these is exempt only when SES says DKIM or DMARC passed: a forged
# From line must not buy a pass.
OWN_SENDER_DOMAINS = ("hams.com",)

# Health-supplement / "secret" / "researchers" clickbait subjects. A weak signal only (weight 1, counted once):
# real mail can say "natural" or "researchers".
CLICKBAIT_SUBJECT_PATTERNS = re.compile(
    r"\bsecret\s+(behind|of|to)\b|\bresearchers?\b|\bdoctors?\b|\bnatural\s+(nutritional|evening|remedy|compound|ingredient|approach)"
    r"|\bsimple\s+(kitchen|ritual|trick|habit)|\bwhat\s+to\s+know\s+about\b|\bbefore\s+(it'?s\s+)?(deleted|banned|removed)"
    r"|\b(memory|neural|energy)\s+(loss|decline)\b|\bsupplement\b|\bmiracle\b|\bone\s+weird\s+trick\b",
    re.IGNORECASE,
)

_UNSUBSCRIBE_RE = re.compile(r"\bunsubscrib", re.IGNORECASE)
_VIEW_IN_BROWSER_RE = re.compile(
    r"view\s+(this\s+)?(e-?mail\s+)?(in|on)\s+(your\s+)?(web\s+)?browser|view\s+online|trouble\s+(viewing|reading)\s+this",
    re.IGNORECASE,
)
_PREFERENCES_RE = re.compile(
    r"(update|manage|change)\s+(your\s+)?(e-?mail\s+)?(preferences|subscriptions?)"
    r"|you\s+(are|'re)\s+(receiving|getting)\s+this\s+(e-?mail|message)|you\s+received\s+this\s+(e-?mail|message)\s+because",
    re.IGNORECASE,
)

_SIGNAL_TEXT_LIMIT = 400000  # characters of HTML the shape parser reads; a hostile message may be far larger


_SHAPE_TAG_RE = re.compile(r"<(/?)(table|img)\b([^>]*)>", re.IGNORECASE)
_SIZE_ATTR_RE = re.compile(r"""\b(?:width|height)\s*=\s*["']?\s*([0-9]+)\s*(?:px)?\s*["'\s/>]""", re.IGNORECASE)


# [@ANCHOR: hams_helpdesk:inbound_html_shape]
def html_shape(html):
    """{"tables", "table_depth", "images", "tracking_pixels"} of an HTML string, counted from its tags: tables and the deepest
    nesting of them, images, and images declared 0 or 1 pixel wide or high. Unbalanced markup never makes the depth negative;
    anything that is not a tag is ignored, so it cannot raise. Reads at most 400,000 characters."""
    tables = depth = max_depth = images = pixels = 0
    for closing, tag, attributes in _SHAPE_TAG_RE.findall((html or "")[:_SIGNAL_TEXT_LIMIT]):
        if tag.lower() == "table":
            if closing:
                depth = max(0, depth - 1)
            else:
                tables += 1
                depth += 1
                max_depth = max(max_depth, depth)
        elif not closing:
            images += 1
            if any(size in ("0", "1") for size in _SIZE_ATTR_RE.findall(attributes + " ")):
                pixels += 1
    return {"tables": tables, "table_depth": max_depth, "images": images, "tracking_pixels": pixels}


# [@ANCHOR: hams_helpdesk:inbound_mail_signals]
def mail_signals(message):
    """Header facts and HTML shape of one parsed inbound ``email.message.Message``, as a small JSON-able dict that
    detect_bulk_mail_signals() and own_authenticated_sender() read. Never raises; an unreadable part contributes
    nothing. Keys: list_unsubscribe (bool), list_id (bool), precedence (str), auto_submitted (str), spf / dkim /
    dmarc ("pass", "fail", ... or absent), from_domain, return_path_domain, reply_headers (bool: In-Reply-To or
    References present), has_plain (bool), plain_chars, html_chars, tables, table_depth, images, tracking_pixels."""
    signals = {}

    def domain_of(address_header):
        _name, address = parseaddr(str(address_header or ""))
        return address.rpartition("@")[2].strip().lower()[:255] if "@" in address else ""

    try:
        signals["list_unsubscribe"] = bool(message.get("List-Unsubscribe"))
        signals["list_id"] = bool(message.get("List-Id"))
        signals["precedence"] = str(message.get("Precedence") or "").strip().lower()[:16]
        signals["auto_submitted"] = str(message.get("Auto-Submitted") or "").strip().lower()[:32]
        signals["reply_headers"] = bool(message.get("In-Reply-To") or message.get("References"))
        signals["from_domain"] = domain_of(message.get("From"))
        signals["return_path_domain"] = domain_of(message.get("Return-Path"))
        # Only the Authentication-Results header written by Amazon SES (authserv-id amazonses.com) is believed: the sender
        # controls any other copy.
        for value in message.get_all("Authentication-Results") or []:
            text = str(value)
            if text.split(";", 1)[0].strip().lower() != "amazonses.com":
                continue
            for method in ("spf", "dkim", "dmarc"):
                match = re.search(r"(?:^|[;\s])%s=(\w+)" % method, text, re.IGNORECASE)
                if match:
                    signals[method] = match.group(1).lower()[:16]
            break
        plain_chars = html_chars = 0
        has_plain = False
        shape = {"tables": 0, "table_depth": 0, "images": 0, "tracking_pixels": 0}
        for part in message.walk():
            if part.is_multipart() or part.get_filename():
                continue
            ctype = part.get_content_type()
            if ctype not in ("text/plain", "text/html"):
                continue
            payload = part.get_payload(decode=True) or b""
            text = payload[:_SIGNAL_TEXT_LIMIT].decode(part.get_content_charset() or "utf-8", "replace")
            if ctype == "text/plain":
                has_plain = True
                plain_chars += len(text.strip())
            else:
                html_chars += len(text)
                part_shape = html_shape(text)
                for key in ("tables", "images", "tracking_pixels"):
                    shape[key] += part_shape[key]
                shape["table_depth"] = max(shape["table_depth"], part_shape["table_depth"])
        signals.update(shape, has_plain=has_plain, plain_chars=plain_chars, html_chars=html_chars)
    except Exception as error:  # audit-ignore-catch-all: a hostile message is the input this must tolerate; partial signals are used, the mail is never dropped
        _logger.info("Inbound mail signals could only be partly read (%s); the mail is still processed.", type(error).__name__)
    return signals


# [@ANCHOR: hams_helpdesk:inbound_own_authenticated_sender]
def own_authenticated_sender(signals):
    """True when the message claims one of OWN_SENDER_DOMAINS in its From line AND SES recorded a DKIM or DMARC pass.
    Our own systems (monitoring, the inbound path check) are exempt from the bulk rules this way; a forged From line
    is not, because it fails authentication."""
    signals = signals or {}
    domain = signals.get("from_domain") or ""
    ours = any(domain == own or domain.endswith("." + own) for own in OWN_SENDER_DOMAINS)
    return ours and "pass" in (signals.get("dkim"), signals.get("dmarc"))


# [@ANCHOR: hams_helpdesk:inbound_bulk_mail_findings]
def bulk_mail_findings(subject, body_html, signals=None):
    """[(weight, reason), ...] for every bulk/marketing signal present; the caller applies the threshold. ``signals`` is
    the mail_signals() dict, or None/{} when only the stored subject and body exist (the retroactive production pass),
    in which case the header and plain-text rules cannot fire and the shape comes from the body itself."""
    signals = signals or {}
    subject = subject or ""
    body_html = (body_html or "")[:_SIGNAL_TEXT_LIMIT]
    findings = []

    if signals.get("list_unsubscribe"):
        findings.append((2, "has a List-Unsubscribe header (bulk mailer)"))
    if signals.get("precedence") in ("bulk", "list", "junk"):
        findings.append((2, "Precedence: %s header (bulk mailer)" % signals["precedence"]))
    if signals.get("list_id"):
        findings.append((1, "has a List-Id header (mailing list)"))
    auto_submitted = signals.get("auto_submitted")
    if auto_submitted and auto_submitted != "no":
        findings.append((1, "Auto-Submitted: %s header (machine-generated)" % auto_submitted))
    if signals.get("dmarc") == "fail":
        findings.append((2, "SES recorded a DMARC failure for the sender"))
    if signals.get("spf") == "fail":
        findings.append((1, "SES recorded an SPF failure for the sender"))
    if signals.get("dkim") == "fail":
        findings.append((1, "SES recorded a DKIM failure for the sender"))
    from_domain = signals.get("from_domain")
    path_domain = signals.get("return_path_domain")
    # Organisational domain: the last two labels, enough to treat a bounce sub-host as the same sender.
    if from_domain and path_domain and ".".join(from_domain.split(".")[-2:]) != ".".join(path_domain.split(".")[-2:]):
        findings.append((1, "Return-Path domain %s is not aligned with the From domain %s" % (path_domain, from_domain)))

    # Marketing HTML shape: from the raw MIME when we have it (images and pixels survive there), else from the body.
    if signals.get("html_chars"):
        shape = signals
        if not signals.get("has_plain"):
            findings.append((1, "HTML message with no plain-text alternative"))
        elif signals.get("plain_chars", 0) < 200 and signals["html_chars"] > 3000:
            findings.append((1, "HTML message whose plain-text alternative is almost empty"))
    else:
        shape = html_shape(body_html)
    if shape.get("tables", 0) >= 4 or shape.get("table_depth", 0) >= 3:
        findings.append((1, "layout built from %d tables nested %d deep (newsletter template)" % (shape.get("tables", 0), shape.get("table_depth", 0))))
    if shape.get("images", 0) >= 3:
        findings.append((1, "%d images (newsletter template)" % shape["images"]))
    if shape.get("tracking_pixels", 0):
        findings.append((1, "contains a 1-pixel tracking image"))

    footer = []
    # The List-Unsubscribe header and the unsubscribe footer are one fact (a bulk mailer), counted once.
    if not signals.get("list_unsubscribe") and _UNSUBSCRIBE_RE.search(body_html):
        footer.append((2, "unsubscribe footer"))
    if _VIEW_IN_BROWSER_RE.search(body_html):
        footer.append((1, "\"view in browser\" footer"))
    if _PREFERENCES_RE.search(body_html):
        footer.append((1, "\"update your preferences\" / \"you are receiving this because\" footer"))
    findings.extend(footer)

    if CLICKBAIT_SUBJECT_PATTERNS.search(subject):
        findings.append((1, "subject in the health-supplement / \"secret\" / \"researchers\" clickbait style"))
    return findings


# [@ANCHOR: hams_helpdesk:inbound_detect_bulk_mail_signals]
def detect_bulk_mail_signals(subject, body_html, signals=None, threshold=BULK_SCORE_THRESHOLD):
    """Plain-English reasons when the bulk/marketing signals weigh at least ``threshold``, else []. The first
    reason states the total; the rest list each contributing signal with its weight. A reply to an existing thread
    (In-Reply-To or References present) is never judged by these rules."""
    if (signals or {}).get("reply_headers"):
        return []
    findings = bulk_mail_findings(subject, body_html, signals)
    total = sum(weight for weight, _reason in findings)
    if total < threshold:
        return []
    return ["bulk/marketing mail: weak signals total %d (flag threshold %d)" % (total, threshold)] + [
        "bulk signal (+%d): %s" % (weight, reason) for weight, reason in findings
    ]


# [@ANCHOR: hams_helpdesk:inbound_spam_reasons]
def inbound_spam_reasons(subject, body_html, signals=None, verdicts=None, trusted_sender=False):
    """The single entry point both inbound routes call: every reason this message should be quarantined, [] when it
    should not. Order: exemptions first (a trusted sender or one of our own authenticated senders is never flagged),
    then the strong rules (detect_inbound_spam_signals: subject patterns and brand impersonation; SES spam/virus
    FAIL), then the weighted bulk/marketing rules. ``trusted_sender`` is honoured only when SES did not record a
    DMARC failure, so a spoofed member address does not bypass the filter."""
    signals = signals or {}
    if own_authenticated_sender(signals):
        return []
    if trusted_sender and signals.get("dmarc") != "fail":
        return []
    return (
        detect_inbound_spam_signals(subject, body_html)
        + ses_verdict_reasons(verdicts)
        + detect_bulk_mail_signals(subject, body_html, signals)
    )
