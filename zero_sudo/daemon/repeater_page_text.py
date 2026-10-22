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

- `section_scope_for_frequency(page_text, freq)` and `callsign_adjacent(page_text, callsign, freq)` (2026-10-22, rasconline.net): the
  deterministic reading of WHERE on the page a repeater's frequency stands. Headings such as 'Other Local Repeaters' or 'Local and
  Regional Repeaters' (and a few translations) mark every row under them as another club's, and a callsign the model applied to a row must be in
  that row, that block, or the heading or intro of its section, not borrowed from a different block. The page text is the cleaned text
  of polite_fetcher.clean_for_model (one line per block-level element, one line per table row, cells joined by ' | ').

- `frequency_on_page`, `number_on_page`, `ground_item`, `grounded_notes` (2026-10-22, 9y4.tt): the grounding check. A repeater the model returned
  from a page that does not state its frequency was invented (the unedited WordPress sample page "Repeaters in the Caribbean" produced three
  repeaters); a frequency, tone or callsign counts as stated only if it is in the cleaned page text, compared as numbers (147.93 is 147.930
  is 147930 kHz).
- `placeholder_page(page_text)`: an unedited default page (the WordPress sample page, "Lorem ipsum", "Coming soon", a web server's default
  page, a parked domain) whose placeholder text is most of the page has no repeaters to extract.

- `orbiting_repeater(...)` (2026-10-22, bdxc.org and crecj.org): whether an extracted "repeater" is the ISS cross-band repeater (ARISS) or an amateur
  satellite, which is a satellite and not a terrestrial repeater, from the item's own name, notes, section, callsign and source address.

MUST NOT import anything from `odoo`. Text is read through glyph_normalizer first, so a slashed zero in the page is a zero here.
"""
import bisect
import functools
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


# [@ANCHOR: zero_sudo:repeater_page_text_month]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
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


# [@ANCHOR: zero_sudo:repeater_page_text_clean]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
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


# [@ANCHOR: zero_sudo:repeater_page_text_to_date]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
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


# ---- where on the page a repeater stands -------------------------------------------------------------------------------

# Headings that set the repeaters below them apart from the club's own. STRONG ones always count; WEAK ones ("Area Repeaters") only on
# a page that also has a club section of its own, because a page whose only list is headed "Area Repeaters" is the club's list.
_OTHER_STRONG = tuple(re.compile(p) for p in (
    r"^(?:other|nearby|neighbou?ring|surrounding|adjacent)\b.*\b(?:repeaters?|relais)\b",
    r"^other local\b",
    r"^useful\b.*\brepeaters?\b",
    r"\blinked (?:repeater )?systems?\b",
    r"^(?:local|regional)\s*(?:and|&|/)\s*(?:regional|local)\b",
    r"\bother clubs?['\u2019]?s?\b.*\brepeaters?\b",
    r"^(?:andere|weitere|benachbarte|umliegende)\b.*\b(?:relais|repeater|umsetzer)",
    r"^otros\b.*\brepetidores",
    r"^(?:d['\u2019]\s?)?autres\b.*\brelais",
    r"^altri\b.*\bripetitori",
))
_OTHER_WEAK = tuple(re.compile(p) for p in (
    r"^(?:area|regional|surrounding)\s+repeaters?$",
    r"^repeaters?\s+(?:in|around)\s+(?:the\s+)?(?:area|region|vicinity|neighbou?rhood)$",
    r"^repeaters?\s+nearby$",
))
_CLUB_OWN = re.compile(r"^(?:our|my|club|member|the club['\u2019]?s?)\b.*\brepeaters?\b")
_CLUB_TITLE = re.compile(r"\brepeaters?\b")
_NOT_A_TITLE_START = ("the ", "a ", "an ", "this ", "these ", "it ", "we ", "if ", "all ", "there ", "please ")
_FREQUENCY = re.compile(r"(?<![\d.])(\d{2,4}\.\d{1,5})(?!\d)(?!\s*hz)", re.I)
_CALL_SIGN_LABEL = re.compile(r"^call\s*sign\s*:", re.I)
_FREQUENCY_TOLERANCE = 0.00051
_INTRO_LINES = 3


# [@ANCHOR: zero_sudo:repeater_page_text_heading_kind]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _heading_kind(line):
    """'other', 'weak_other', 'club' or None for one line of page text. Only a short line without a sentence or a label in it can
    be a heading; a lone word 'Repeaters' (a page title or a menu entry) is none."""
    text = line.strip(" *#-\u2013\u2014\t").rstrip(":").strip()
    if not text or len(text) > 80 or " | " in text or ":" in text or text.endswith((".", "!", "?")):
        return None
    low = text.lower()
    if any(p.search(low) for p in _OTHER_STRONG):
        return "other"
    if any(p.match(low) for p in _OTHER_WEAK):
        return "weak_other"
    if len(text) <= 60 and not low.startswith(_NOT_A_TITLE_START) and (
            _CLUB_OWN.search(low) or (_CLUB_TITLE.search(low) and len(low.split()) >= 2)):
        return "club"
    return None


# [@ANCHOR: zero_sudo:repeater_page_text_title]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _title(line):
    return line.strip(" *#-\u2013\u2014\t").rstrip(":").strip().lower()


# [@ANCHOR: zero_sudo:repeater_page_text_frequencies]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _frequencies(line):
    return [float(m.group(1)) for m in _FREQUENCY.finditer(line)]


# [@ANCHOR: zero_sudo:repeater_page_text_structure]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
@functools.lru_cache(maxsize=8)
def _structure(page_text):
    """The page as lines with, for each: its kind (row, heading or text), the scope of the section it stands in ('club', 'other' or
    None), the index of that section's heading line, and whether the section is a detail block (has a 'Call Sign:' line)."""
    lines = page_text.split("\n")
    kinds = []
    for line in lines:
        kind = _heading_kind(line)
        kinds.append("row" if " | " in line else kind or "text")
    scope, heading_at = [], []
    current = None
    for index, kind in enumerate(kinds):
        if kind == "weak_other":
            kind = "other"  # provisional: reverted below when the page has no section of the club's own
        if kind in ("other", "club"):
            current = (kind, index)
        scope.append(current[0] if current else None)
        heading_at.append(current[1] if current else None)
    detail = {}
    for index, line in enumerate(lines):
        if heading_at[index] is not None and scope[index] == "club" and _CALL_SIGN_LABEL.match(line.strip()):
            detail[heading_at[index]] = True
    own = {i for i, k in enumerate(kinds) if k == "club" and (_CLUB_OWN.search(_title(lines[i])) or i in detail)}
    if not own:  # a page with no section of the club's own: "Area Repeaters" is its only list, not another club's
        weak = [i for i, line in enumerate(lines) if _heading_kind(line) == "weak_other"]
        if weak:
            kinds = [("text" if i in weak else k) for i, k in enumerate(kinds)]
            scope, heading_at, current = [], [], None
            for index, kind in enumerate(kinds):
                if kind in ("other", "club"):
                    current = (kind, index)
                scope.append(current[0] if current else None)
                heading_at.append(current[1] if current else None)
    return lines, kinds, scope, heading_at, detail, own


# [@ANCHOR: zero_sudo:repeater_page_text_occurrences]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _occurrences(page_text, freq):
    lines = _structure(page_text)[0]
    return [i for i, line in enumerate(lines) if any(abs(f - freq) < _FREQUENCY_TOLERANCE for f in _frequencies(line))]


# [@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def section_scope_for_frequency(page_text, freq):
    """`(scope, heading)` for the repeater on `freq` (MHz): 'other' when the page lists it under a heading that sets it apart from the
    club's own repeaters ('Other Local Repeaters', 'Local and Regional Repeaters', 'Nearby Repeaters', ...), 'club' when it stands in
    a section of the club's own ('Our Repeaters', 'RASC VHF Repeater' with its Call Sign line), else None, with the heading text. The more
    specific place wins when the page puts the frequency in two (its own table row or a detail block beats a heading), and a tie is
    None, left to the model. Never raises."""
    if not isinstance(page_text, str) or not isinstance(freq, (int, float)) or isinstance(freq, bool):
        return None, ""
    lines, kinds, scope, heading_at, detail, own = _structure(page_text)
    found = {}
    for index in _occurrences(page_text, float(freq)):
        if scope[index] is None:
            continue
        specificity = 2 if kinds[index] == "row" or heading_at[index] in detail else 1
        # a generic title ("Tehachapi Area Repeaters") says nothing about whose the repeaters are: it competes, and says "unknown"
        kind = "generic" if scope[index] == "club" and heading_at[index] not in own else scope[index]
        found.setdefault(specificity, set()).add((kind, heading_at[index]))
    if not found:
        return None, ""
    best = found[max(found)]
    scopes = {s for s, _h in best}
    if len(scopes) != 1 or "generic" in scopes:
        return None, ""
    heading = lines[min(h for _s, h in best)].strip(" *#-\u2013\u2014\t").rstrip(":").strip()
    return scopes.pop(), heading


# [@ANCHOR: zero_sudo:repeater_page_text_own_span]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _own_span(page_text, occurrences):
    """The line indexes that are the repeater's own: its row or line, its detail block, and the heading and intro lines of its section."""
    lines, kinds, scope, heading_at, detail, own = _structure(page_text)
    span = set()
    for index in occurrences:
        span.add(index)
        heading = heading_at[index]
        if heading is None:
            continue
        span.add(heading)
        if kinds[index] != "row" and heading in detail:
            span.update(i for i in range(len(lines)) if heading_at[i] == heading)
        following = heading + 1
        while following < len(lines) and following - heading <= _INTRO_LINES and kinds[following] == "text" and not _frequencies(lines[following]):
            span.add(following)
            following += 1
    return span


# [@ANCHOR: zero_sudo:repeater_page_text_callsign_adjacent]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_adjacency]
def callsign_adjacent(page_text, callsign, freq):
    """Whether `callsign` belongs to the repeater on `freq` by where it stands on the page: True when it is in that repeater's own row
    or detail block, or in the heading or intro lines of the section the row is in, or on a line that belongs to no repeater block
    (a page header or footer, the organisation's callsign); False when it is found ONLY inside some other repeater's row, block or
    section (N7CRA from the Cultus Mountain block applied to a row of another table); None when the frequency is not found on the
    page or the callsign is nowhere in it, so nothing can be said. Glyph-repaired, tolerant of whitespace. Never raises."""
    if not isinstance(page_text, str) or not isinstance(freq, (int, float)) or isinstance(freq, bool) or not isinstance(callsign, str):
        return None
    wanted = strip_repeater_designator(_clean(callsign))[0]
    lines, kinds, scope, heading_at, detail, own = _structure(page_text)
    occurrences = _occurrences(page_text, float(freq))
    if not wanted or not occurrences:
        return None
    own = _own_span(page_text, occurrences)
    if any(wanted in callsigns_in_text(lines[i]) for i in own):
        return True
    tied = {i for i in range(len(lines)) if kinds[i] == "row" or scope[i] is not None or _frequencies(lines[i])}
    elsewhere = [i for i in range(len(lines)) if i not in own and wanted in callsigns_in_text(lines[i])]
    if not elsewhere:
        return None
    return True if any(i not in tied for i in elsewhere) else False


# ---- grounding: is it on the page? --------------------------------------------------------------------------------------

_NUMBER_TOKEN = re.compile(r"(?<![\d.,])\d[\d.,]*\d|\d")
_NUMBER_TOLERANCE = 0.00051
_TONE_TOLERANCE = 0.06
_UNIT_CLAIM = re.compile(r"(\d[\d,.]*)\s*(?:w|watts?|kw|kilowatts?|miles?|mi|km|kilometers?|ft|feet|foot|meters?)\b", re.I)
_AGENCY_WORDS = (
    "u.s. navy", "navy", "army", "air force", "marines", "coast guard", "national guard", "fema", "red cross", "noaa", "skywarn",
    "department of defense", "homeland security", "sheriff", "police", "fire department", "hospital",
)


# [@ANCHOR: zero_sudo:repeater_page_text_page_numbers]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
@functools.lru_cache(maxsize=8)
def _page_numbers(page_text):
    """Every number the page states, as floats, in every reading a frequency might take: as written, with a decimal comma, with
    thousands separators removed, a kHz value (147930) as MHz and a Hz value as MHz. Sorted."""
    clean = glyph_normalizer.normalize_number_text(page_text)[0] if isinstance(page_text, str) else ""
    found = set()
    for token in _NUMBER_TOKEN.findall(clean):
        token = token.strip(".,")
        readings = {token}
        if token.count(",") == 1 and "." not in token:
            readings.add(token.replace(",", "."))
        readings.add(token.replace(",", ""))
        for reading in readings:
            try:
                value = float(reading)
            except ValueError:
                continue
            found.add(value)
            if 1000 <= value < 1e7:
                found.add(value / 1000.0)
            elif value >= 1e7:
                found.add(value / 1e6)
    return tuple(sorted(found))


# [@ANCHOR: zero_sudo:repeater_page_text_near]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _near(values, wanted, tolerance):
    at = bisect.bisect_left(values, wanted - tolerance)
    return at < len(values) and values[at] <= wanted + tolerance


# [@ANCHOR: zero_sudo:repeater_page_text_frequency_on_page]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_grounding]
def frequency_on_page(page_text, freq):
    """True when the page states `freq` (MHz): as 147.93, 147.930, 147.9300, 147,93 or 147930 kHz. Never raises."""
    if not isinstance(freq, (int, float)) or isinstance(freq, bool):
        return False
    return _near(_page_numbers(page_text), float(freq), _NUMBER_TOLERANCE)


# [@ANCHOR: zero_sudo:repeater_page_text_number_on_page]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_grounding]
def number_on_page(page_text, value, tolerance=_TONE_TOLERANCE):
    """True when the page states a number within `tolerance` of `value` (a CTCSS tone, say). Never raises."""
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        return False
    return _near(_page_numbers(page_text), float(value), tolerance)


# [@ANCHOR: zero_sudo:repeater_page_text_grounded_notes]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_grounding]
def grounded_notes(notes, page_text):
    """`(kept, dropped)`: the sentences of `notes` that make no specific claim the page does not back, and those that do. A claim is an
    amount with a unit (50 W, 30 miles) whose number is not on the page, or an agency or service (Navy, FEMA, Red Cross) the page never
    names. Anything else in the notes is paraphrase and is kept. Never raises."""
    if not isinstance(notes, str) or not notes.strip():
        return "", []
    low = page_text.lower() if isinstance(page_text, str) else ""
    kept, dropped = [], []
    for sentence in re.split(r"(?<![A-Z]\.)(?<=[.!?])\s+", notes.strip()):
        unsupported = any(not number_on_page(page_text, float(m.group(1).replace(",", "")), 0.001)
                          for m in _UNIT_CLAIM.finditer(sentence) if re.fullmatch(r"[\d,]+(?:\.\d+)?", m.group(1)))
        unsupported = unsupported or any(re.search(rf"(?<![a-z]){re.escape(w)}(?![a-z])", sentence.lower()) and w not in low
                                         for w in _AGENCY_WORDS)
        (dropped if unsupported else kept).append(sentence)
    return " ".join(kept), dropped


# [@ANCHOR: zero_sudo:repeater_page_text_ground_item]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_grounding]
def ground_item(item, page_text):
    """`(item, problems)`: a copy of one extracted repeater checked against the page text. An output frequency the page does not state
    makes it `not_on_page` (the key holds a plain reason) and drops its notes: it was not read from this page. Otherwise an input
    frequency, tone or callsign the page does not state is removed, and notes sentences with a claim the page does not back are dropped.
    `problems` lists what was done, for the item's notes. A non-dict is returned unchanged. Never raises."""
    if not isinstance(item, dict):
        return item, []
    item = dict(item)
    problems = []
    output = item.get("output_frequency")
    if isinstance(output, (int, float)) and not isinstance(output, bool) and not frequency_on_page(page_text, output):
        item["not_on_page"] = "the extracted frequency is not stated on the page (possibly invented by the model)"
        item.pop("notes", None)
        return item, [f"output frequency {output:g} MHz is not on the page"]
    for key in ("input_frequency",):
        value = item.get(key)
        if isinstance(value, (int, float)) and not isinstance(value, bool) and not frequency_on_page(page_text, value):
            item.pop(key)
            problems.append(f"{key} {value:g} is not on the page; removed")
    for key in ("tone_uplink", "tone_downlink"):
        value = item.get(key)
        if isinstance(value, (int, float)) and not isinstance(value, bool) and not number_on_page(page_text, value):
            item.pop(key)
            problems.append(f"{key} {value:g} is not on the page; removed")
    callsign = item.get("callsign")
    if isinstance(callsign, str):
        base = strip_repeater_designator(_clean(callsign))[0]
        if _BASE_CALLSIGN.match(base) and base not in callsigns_in_text(page_text):
            item.pop("callsign")
            problems.append(f"callsign {callsign} is not on the page; removed")
    if isinstance(item.get("notes"), str):
        kept, dropped = grounded_notes(item["notes"], page_text)
        if dropped:
            item["notes"] = kept
            if not kept:
                item.pop("notes")
            problems.append(f"{len(dropped)} note sentence(s) with claims the page does not back removed")
    return item, problems


# ---- placeholder pages ----------------------------------------------------------------------------------------------------

_PLACEHOLDER_PATTERNS = tuple(re.compile(p, re.I) for p in (
    r"this is an example page", r"as a new wordpress user,? you should go to your dashboard", r"hello world!?\s*welcome to wordpress",
    r"this is your first post", r"just another wordpress site", r"^sample page$", r"bike messenger by day", r"xyz doohickey company",
    r"lorem ipsum", r"\byour content here\b", r"^coming soon!?$", r"\bwebsite coming soon\b", r"\bunder construction\b",
    r"welcome to your new (?:site|website)", r"this is a placeholder", r"default web site page", r"^it works!?$",
    r"apache2? (?:ubuntu |debian )?default page", r"test page for the apache", r"welcome to nginx", r"internet information services",
    r"\b(?:this )?domain (?:is )?(?:parked|for sale)\b", r"\bbuy this domain\b", r"\bsite (?:is )?(?:temporarily )?unavailable\b",
    r"\bthis site is (?:currently )?being built\b", r"\bstart editing (?:your|this) (?:site|page)\b",
))
_FOOTER_WORDS = re.compile(r"copyright|\u00a9|powered by|all rights reserved|privacy policy", re.I)
_MAIN_LINE = 30
_SHORT_PAGE = 160


# [@ANCHOR: zero_sudo:repeater_page_text_placeholder_page]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_placeholder]
def placeholder_page(page_text):
    """The placeholder phrase that makes `page_text` an unedited default page, or '' for a real page. The page is placeholder only when
    the lines carrying placeholder text are most (half or more) of its main text (lines of 30 characters or more, footers apart), or the
    whole main text is under 160 characters, and no other line states a frequency. A page that merely mentions "under construction" in
    real content is not a placeholder. Never raises."""
    if not isinstance(page_text, str):
        return ""
    lines = [line.strip().replace("\u2019", "'") for line in page_text.split("\n") if line.strip()]
    main = [line for line in lines if len(line) >= _MAIN_LINE and not _FOOTER_WORDS.search(line)]
    first = ""
    matched = 0
    for line in lines:
        hit = next((p.search(line) for p in _PLACEHOLDER_PATTERNS if p.search(line)), None)
        if hit:
            first = first or hit.group(0)
            if line in main:
                matched += len(line)
            elif len(line) < _MAIN_LINE:
                continue
    if not first:
        return ""
    if any(_frequencies(line) for line in lines):  # a page that states a frequency is not a default page
        return ""
    total = sum(len(line) for line in main)
    if total <= _SHORT_PAGE or matched * 2 >= total:
        return first
    return ""


# ---- whose is it? ownership evidence -------------------------------------------------------------------------------------

_SSID = re.compile(r"-(?:[1-9]|1[0-5])$")
_FIRST_PERSON = re.compile(r"\b(?:our|we (?:own|operate|maintain|run|sponsor))\b", re.I)
_OPERATED_BY = re.compile(r"\b(?:operated|owned|maintained|sponsored|run) by ([^.;|\u00b7]{3,80})", re.I)
_GENERIC_WORDS = frozenset(
    "the and amateur radio club association assoc society group repeater repeaters system network county valley mountain "
    "emergency communications service services of for ham".split())


# [@ANCHOR: zero_sudo:repeater_page_text_callsign_core]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_ownership]
def callsign_core(callsign):
    """A callsign for COMPARISON only: upper case, glyph-repaired, a repeater designator (/R) and an APRS SSID (-1 to -15) removed, so
    'AC6EE-3' and 'ac6ee/R' are the station AC6EE. Whatever the page wrote is what gets stored; this is never stored. Never raises."""
    base = strip_repeater_designator(_clean(callsign))[0]
    return _SSID.sub("", base)


# [@ANCHOR: zero_sudo:repeater_page_text_header_callsigns]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_ownership]
def header_callsigns(page_text):
    """The callsigns the page states as its OWN ('TARA AC6EE' in the title bar, '73 de AC6EE' and a copyright line at the foot): those that
    are in the first five lines and again in the last eight. A callsign in a row or a block is not. Never raises."""
    if not isinstance(page_text, str):
        return set()
    lines = [line for line in page_text.split("\n") if line.strip()]
    if len(lines) < 12:
        return set()
    per_line = [set() if " | " in line else {callsign_core(c) for c in callsigns_in_text(line)} for line in lines]
    found = set()
    for core in set().union(*per_line) if per_line else set():
        where = [i for i, calls in enumerate(per_line) if core in calls]
        if any(i < 5 for i in where) and any(i >= len(lines) - 8 for i in where):
            found.add(core)
    return found


# [@ANCHOR: zero_sudo:repeater_page_text_title_words]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _title_words(page_text):
    lines = [line for line in page_text.split("\n") if line.strip() and " | " not in line and not _frequencies(line)][:4]
    return {w.lower() for line in lines if len(line) < 90 for w in re.findall(r"[A-Za-z]{3,}", line)} - _GENERIC_WORDS


# [@ANCHOR: zero_sudo:repeater_page_text_row_ownership]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_ownership]
def row_ownership(page_text, freq, callsign=None):
    """'club', 'other' or None (no evidence) for the repeater on `freq`, from the page alone: 'club' when it stands in a section of the
    club's own or a detail block, its callsign is one the page states as its own (the title bar, the footer), or its row or block says
    'our' or 'operated by' the club; 'other' when it is under a heading that sets it apart from the club's ('Other Local Repeaters',
    'Useful ... Repeaters', 'KERN Linked Repeater System') or its row or block says 'operated by' someone else. A list that is only a
    generic title ('Tehachapi Area Repeaters') gives no evidence. Never raises."""
    scope, _heading = section_scope_for_frequency(page_text, freq)
    if scope in ("club", "other"):
        return scope
    if not isinstance(page_text, str):
        return None
    lines = _structure(page_text)[0]
    span = " ".join(lines[i] for i in sorted(_own_span(page_text, _occurrences(page_text, float(freq)))))
    if isinstance(callsign, str) and callsign_core(callsign) in header_callsigns(page_text):
        return "club"
    if _FIRST_PERSON.search(span):
        return "club"
    named = _OPERATED_BY.search(span)
    if named:
        text = named.group(1)
        mine = bool({callsign_core(c) for c in callsigns_in_text(text)} & header_callsigns(page_text)) or bool(
            {w.lower() for w in re.findall(r"[A-Za-z]{3,}", text)} & _title_words(page_text))
        return "club" if mine else "other"
    return None


# [@ANCHOR: zero_sudo:repeater_page_text_append_note]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _append_note(item, text):
    notes = item.get("notes")
    item["notes"] = f"{text}. {notes}" if isinstance(notes, str) and notes else text


# [@ANCHOR: zero_sudo:repeater_page_text_item_frequency]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
def _item_frequency(item, page_text):
    for key in ("output_frequency", "input_frequency"):
        value = item.get(key)
        if isinstance(value, (int, float)) and not isinstance(value, bool) and _occurrences(page_text, float(value)):
            return float(value)
    return None


# [@ANCHOR: zero_sudo:repeater_page_text_apply_page_checks]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_ownership]
def apply_page_checks(items, page_text):
    """`items` (extracted repeaters) with what the page's own structure says about each, decided in code whatever the model returned:

    - `listing_scope`/`listing_section`: 'other' when the page lists the frequency under a heading that sets it apart from the club's own
      repeaters (the model may make this stricter, club to other, never looser); 'club' when it stands in a section of the club's own and the
      model gave none.
    - a callsign found only in some OTHER repeater's row, block or section is removed with the note 'callsign from another section removed';
    - `listing_ownership`: 'club' / 'other' (row_ownership), else 'unclear' when the page lists two or more callsigns with no evidence of
      whose they are, else 'unspecified'. Only an item with a callsign is judged.
    Items that are not dicts, and everything when `page_text` is not text, pass through. Never raises."""
    if not isinstance(page_text, str):
        return items
    checked = []
    for item in items:
        if not isinstance(item, dict):
            checked.append(item)
            continue
        item = dict(item)
        freq = _item_frequency(item, page_text)
        if freq is not None:
            scope, heading = section_scope_for_frequency(page_text, freq)
            model = item.get("listing_scope")
            if scope == "other" or (scope == "club" and model not in ("club", "other")):
                item["listing_scope"] = scope
                if heading and not item.get("listing_section"):
                    item["listing_section"] = heading
            callsign = item.get("callsign")
            if isinstance(callsign, str) and callsign_adjacent(page_text, callsign, freq) is False:
                item.pop("callsign")
                _append_note(item, f"callsign from another section removed: {callsign}")
        checked.append(item)
    evidence = {}
    for index, item in enumerate(checked):
        if isinstance(item, dict) and isinstance(item.get("callsign"), str):
            freq = _item_frequency(item, page_text)
            found = row_ownership(page_text, freq, item["callsign"]) if freq is not None else None
            if item.get("listing_scope") == "other":
                found = "other"
            evidence[index] = found
    unknown = {callsign_core(checked[i]["callsign"]) for i, found in evidence.items() if found is None}
    for index, found in evidence.items():
        checked[index]["listing_ownership"] = found or ("unclear" if len(unknown) >= 2 else "unspecified")
    return checked


# ---- repeaters in orbit --------------------------------------------------------------------------------------------------

ISS_CALLSIGNS = frozenset({"NA1SS", "RS0ISS", "OR4ISS", "DP0ISS", "IR0ISS", "LX0ISS", "W5RRR", "N1SS"})
_ISS_FREQUENCIES = (145.990, 145.800, 145.825, 437.800, 437.550)
_SPECIFIC_ORBIT = tuple(re.compile(p, re.I) for p in (
    r"(?<![a-z0-9])ariss(?![a-z0-9])", r"(?<![a-z0-9])iss(?![a-z0-9])", r"international space station", r"\bspace station\b",
    r"estaci[o\u00f3]n espacial", r"station spatiale", r"\braumstation\b", r"esta[c\u00e7][a\u00e3]o espacial", r"stazione spaziale",
    r"(?<![a-z0-9])(?:ao|so|fo|rs|xw|cas|lo|eo)-\d{1,3}[a-z]?(?![a-z0-9.])", r"\bqo-100\b", r"\bes['\u2019]?hail\b", r"\btianhe\b",
    r"\bukube\b", r"\blilacsat\b", r"\bcubesat\b",
))
_OSCAR_NUMBER = re.compile(r"\boscar[\s-]*(\d{1,3})(?![\d]|[.,]\d)", re.I)
_GENERIC_SATELLITE = re.compile(r"sat[e\u00e9]llite|satellit|\bsat\b", re.I)
_ORBIT_CONTEXT = re.compile(r"\b(?:uplinks?|downlinks?|transponders?|orbits?|orbital|amsat|oscar|linear|subida|bajada|cross-?band)\b", re.I)
_UPLINK = re.compile(r"\b(?:uplink|downlink|subida|bajada)\b", re.I)


# [@ANCHOR: zero_sudo:repeater_page_text_orbiting_repeater]
# Verified by [@ANCHOR: zero_sudo:test_repeater_page_text_orbiting]
def orbiting_repeater(name=None, notes=None, section=None, url=None, callsign=None, frequencies=()):
    """The text that shows the extracted item is a repeater IN ORBIT (the ISS cross-band repeater, an amateur satellite or its transponder), or ''.
    Evidence is the item itself: its name, section heading, source address, callsign (NA1SS, RS0ISS, OR4ISS, DP0ISS, IR0ISS), or notes that
    describe an uplink and downlink on the ISS or a satellite. ARISS, ISS as a whole word, 'International Space Station' and its translations,
    satellite designators (AO-91, RS-44, XW-2A..., QO-100, Es'hail, Tianhe, CubeSat) and OSCAR with a satellite number (OSCAR 7, OSCAR-51, never
    OSCAR 145.490, which is a frequency) count in the name, section or address; the plain word 'satellite' counts there only beside uplink,
    downlink, transponder or the like. In the notes only a description of an uplink/downlink on the ISS or a satellite counts, so a terrestrial
    repeater whose notes mention 'FM satellite format', or a club that makes ARISS contacts, is not rejected. Never raises."""
    slug = " ".join(re.split(r"[^A-Za-z0-9']+", url.split("://", 1)[-1].split("/", 1)[-1])) if isinstance(url, str) and "/" in url.split("://", 1)[-1] else ""
    if isinstance(callsign, str) and strip_repeater_designator(_clean(callsign))[0] in ISS_CALLSIGNS:
        return callsign.strip()

    def oscar(text):
        return next((m.group(0) for m in _OSCAR_NUMBER.finditer(text) if int(m.group(1)) <= 129), "")

    def specific(text):
        for pattern in _SPECIFIC_ORBIT:
            found = pattern.search(text)
            if found:
                return found.group(0)
        return oscar(text)

    for text in (name, section, slug):
        if isinstance(text, str) and text.strip():
            found = specific(text)
            if found:
                return found
            if _GENERIC_SATELLITE.search(text) and _ORBIT_CONTEXT.search(text):
                return _GENERIC_SATELLITE.search(text).group(0)
    if isinstance(notes, str) and _UPLINK.search(notes):
        found = specific(notes)
        if found:
            return found
        if _GENERIC_SATELLITE.search(notes):
            return _GENERIC_SATELLITE.search(notes).group(0)
        stated = [f for f in _frequencies(notes)]
        if any(abs(a - b) < _FREQUENCY_TOLERANCE for a in stated for b in _ISS_FREQUENCIES) and len(stated) >= 2:
            return "uplink and downlink on the ISS frequencies"
    return ""
