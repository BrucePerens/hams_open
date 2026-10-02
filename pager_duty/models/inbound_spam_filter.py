# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Real, explainable spam/phishing heuristic for inbound-mail-created
pager.incident records, applied at mail-ingestion time in incident_ticket_
adapter.py's action_generate_helpdesk_ticket() -- per Bruce's own decision
recorded in night_shift_questions/answered/inbound-spam-filter-location-
and-signal-e14a6f8b.md ("Filter at mail-ingestion time... in pager_duty's
own Helpdesk Adapter"), not as a post-hoc hams_helpdesk.ticket stage/tag
and not left to the ticket-triage AI pass.

Deliberately NOT a machine-learning classifier (per the to-do's own "start
simple and defensible" instruction) -- every signal below is a plain,
auditable rule with its own human-readable reason string, so a reviewer
(or a future session) can see exactly why a message was flagged, not just
an opaque score. See night_shift_todo/high/inbound-mail-ticket-ingestion-
has-no-spam-phishing-filter-e3a8f612.md for the real, live spam/phishing
examples this was built from.

False positives are the named risk, not false negatives: this module only
ever classifies, it never deletes or drops anything -- the caller is
responsible for routing a flagged message to a visible quarantine stage
that a human can still review and recover from (see incident_ticket_
adapter.py's own "spam" stage handling).
"""
import re
from urllib.parse import urlsplit

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
        hostname = urlsplit(href).hostname
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
