# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Pure-stdlib, Odoo-free readings of the text of a club's repeater page that the extraction's post-processing needs, shared by the
Odoo import code and the crawl daemons (imported the same way as glyph_normalizer.py, which it builds on):

- `strip_repeater_designator(callsign)`: 'K0HNY/R' is the repeater K0HNY. A trailing /R, -R, /RPT, -RPT, /REP, ' RPT' or ' REP'
  on a callsign-shaped token is a designator for "repeater", not part of the licensed callsign. /P, /M, /QRP, a country prefix
  ('VE3/W1AW', 'W1AW/VE3') and anything not callsign-shaped are left exactly as they are.
- `node_operator_callsigns(text)` and `callsigns_in_text(text)`: a page that says 'EchoLink node 469520 operated by KC0REO' names
  the operator of a VoIP node, who is not the repeater's licensee; the callers use these to refuse that callsign for the repeater
  when the page ties another callsign to it.
- `page_date(text)` and `stale_page_note(text, today)`: the latest date the page states of ITSELF ('Last updated 14-Jan-2021',
  'Revised: January 3, 2019', 'as of 5/6/2022') and, when it is over two years old, the note 'page dated 14-Jan-2021, may be out of
  date'. A date without such a label (an event, a copyright year) is not the page's own.

MUST NOT import anything from `odoo`. Text is read through glyph_normalizer first, so a slashed zero in the page is a zero here.
"""
import re
from datetime import date, timedelta

if __package__:  # imported as odoo.addons.zero_sudo.daemon.repeater_page_text
    from . import glyph_normalizer
else:  # the flat copy in a daemon's directory, a top-level module beside glyph_normalizer.py
    import glyph_normalizer

_BASE_CALLSIGN = re.compile(r"^[A-Z0-9]{1,3}[0-9][A-Z]{1,4}$")
_DESIGNATOR = re.compile(r"^(?P<base>[A-Z0-9]{1,3}[0-9][A-Z]{1,4})(?P<suffix>[/-](?:R|RPT|REP)|\s+(?:RPT|REP))$")
_CALL = r"[A-Za-z0-9]{1,3}[0-9][A-Za-z]{1,4}"
_CALLSIGN_TOKEN = re.compile(rf"(?<![A-Za-z0-9/.-])({_CALL})(?:[/-](?:R|RPT|REP)\b)?(?![A-Za-z0-9])")
_OPERATOR_PATTERNS = (
    re.compile(rf"\bnode\s*(?:#|number|no\.?)?\s*\d{{3,7}}\s*(?:is\s+|are\s+)?(?:operated|run|owned|sponsored|maintained|hosted|provided)\s+by\s+({_CALL})\b", re.I),
    re.compile(rf"\bnode\s+(?:operator|owner|sysop|sponsor)\s*(?:is\b|:)?\s*({_CALL})\b", re.I),
    re.compile(rf"\b({_CALL})\s+(?:operates|runs|hosts|sponsors|maintains)\s+(?:the\s+|this\s+)?(?:echolink\s+|allstar\s+|irlp\s+)?node\b", re.I),
)
_MONTHS = "jan feb mar apr may jun jul aug sep oct nov dec".split()
_MONTH = r"(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*"
_LABEL = r"(?:last\s+)?(?:updated?|revised?|revision|modified|as\s+of|dated?)\s*(?:on\b|:|-)?\s*"
_DATE_FORMS = (
    ("dmy", re.compile(rf"(\d{{1,2}})(?:st|nd|rd|th)?[-\s/.]*({_MONTH})\.?[-\s/.,]*(\d{{4}})", re.I)),
    ("mdy", re.compile(rf"({_MONTH})\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?,?\s+(\d{{4}})", re.I)),
    ("iso", re.compile(r"(\d{4})-(\d{2})-(\d{2})")),
    ("us", re.compile(r"(\d{1,2})/(\d{1,2})/(\d{4})")),
)
_LABELLED = [(kind, re.compile(_LABEL + "(" + form.pattern + ")", re.I)) for kind, form in _DATE_FORMS]
STALE_AFTER = timedelta(days=2 * 365)


def _month(name):
    return _MONTHS.index(name[:3].lower()) + 1


# [@ANCHOR: zero_sudo:repeater_page_text_strip_repeater_designator]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_designator]
def strip_repeater_designator(callsign):
    """`(callsign, suffix)`: the callsign upper-cased with a trailing repeater designator removed, and the designator removed
    ('' when there was none). Only a callsign-shaped base is stripped; /P, /M, /QRP and country prefixes stay. Never raises."""
    text = callsign.strip().upper() if isinstance(callsign, str) else ""
    found = _DESIGNATOR.match(text)
    if found and _BASE_CALLSIGN.match(found.group("base")):
        return found.group("base"), found.group("suffix")
    return text, ""


def _clean(text):
    return glyph_normalizer.normalize_callsign_text(text)[0] if isinstance(text, str) else ""


# [@ANCHOR: zero_sudo:repeater_page_text_node_operator_callsigns]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_node_operators]
def node_operator_callsigns(text):
    """The callsigns `text` names as the operator, owner or sponsor of an EchoLink, AllStar or IRLP node ('node 469520 operated
    by KC0REO', 'node operator: W1AW', 'W1AW runs the node'), upper-cased and glyph-repaired. Never raises."""
    clean = _clean(text)
    found = set()
    for pattern in _OPERATOR_PATTERNS:
        for match in pattern.finditer(clean):
            found.add(strip_repeater_designator(match.group(1))[0])
    return found


# [@ANCHOR: zero_sudo:repeater_page_text_callsigns_in_text]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_node_operators]
def callsigns_in_text(text):
    """Every callsign-shaped token in `text` (glyph-repaired, upper-cased, repeater designators removed). Loose on purpose: it is
    only used to ask whether the page names any callsign besides a node operator's. Never raises."""
    return {strip_repeater_designator(m.group(1))[0] for m in _CALLSIGN_TOKEN.finditer(_clean(text))}


def _to_date(kind, parts):
    try:
        if kind == "dmy":
            return date(int(parts[2]), _month(parts[1]), int(parts[0]))
        if kind == "mdy":
            return date(int(parts[2]), _month(parts[0]), int(parts[1]))
        if kind == "iso":
            return date(int(parts[0]), int(parts[1]), int(parts[2]))
        return date(int(parts[2]), int(parts[0]), int(parts[1]))
    except ValueError:
        return None


# [@ANCHOR: zero_sudo:repeater_page_text_page_date]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_stale]
def page_date(text):
    """`(date, text_as_written)` for the LATEST date `text` states of itself behind a label such as 'Last updated', 'Revised',
    'Modified' or 'as of'; `(None, '')` when there is none or none is a real calendar date. Day-month-year with a month name,
    month name first, year-month-day and month/day/year are read. Digits of any script are repaired first. Never raises."""
    clean = glyph_normalizer.normalize_number_text(text)[0] if isinstance(text, str) else ""
    best = (None, "")
    for kind, pattern in _LABELLED:
        for match in pattern.finditer(clean):
            parts = match.groups()[1:]
            parsed = _to_date(kind, parts)
            if parsed and (best[0] is None or parsed > best[0]):
                best = (parsed, match.group(1).strip())
    return best


# [@ANCHOR: zero_sudo:repeater_page_text_stale_page_note]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_stale]
def stale_page_note(text, today=None):
    """'page dated 14-Jan-2021, may be out of date' when the page states its own date and that date is more than two years
    before `today` (default: today); '' otherwise. A note, never a rejection."""
    stated, written = page_date(text)
    if stated is None or (today or date.today()) - stated <= STALE_AFTER:
        return ""
    return f"page dated {written}, may be out of date"
