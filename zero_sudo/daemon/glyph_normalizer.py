# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Pure-stdlib, Odoo-free clean-up of the odd glyphs web pages use for digits and for letters in callsigns, shared by the
Odoo import code and the standalone crawl daemons (the same arrangement as robots_txt_policy.py and ssrf_safe_fetch.py:
Odoo imports `odoo.addons.zero_sudo.daemon.glyph_normalizer`, the daemons import the flat file that
devbox_tools/deploy_to_production.py syncs beside page_cleaning.py).

Ham text writes a zero as a slashed O (WØAZR), a crossed seven as 7 plus a combining stroke, a dotted zero as 0 plus a
combining dot, and pages are typed with full-width digits, Arabic-Indic digits, mathematical digits, circled and superscript
digits, zero-width characters and non-breaking spaces. None of that is a different number or a different callsign.

Two public functions, each returning `(clean_text, changes)`; `changes` is a list of `(original, "U+XXXX", replacement)`
for every substitution or removal (original is the glyph or glyph cluster, the code is its code point(s), replacement is ''
for a removal), so a caller can write it into the candidate's notes:

- `normalize_callsign_text(text)`
- `normalize_number_text(text)`

Stages, in order: (1) format characters (Cf: zero-width, BOM, soft hyphen, tags...) are removed and every space separator
(NBSP, narrow NBSP...) becomes a plain space; (2) NFKC, character by character (full-width, ligatures, superscript, circled,
mathematical digits); (3) every decimal digit of any script (category Nd, `unicodedata.decimal`) and every digit-valued No
character (`unicodedata.digit`) becomes its ASCII digit; (4) a combining mark on an ASCII digit (the stroke of a crossed
seven, the slash of 0 + U+0338, the dot of a dotted zero) is removed, and a letter O carrying a stroke overlay
(U+0335-U+0338) counts as a slashed zero; (5) the table of zero-like precomposed glyphs below; (6) callsigns only: the
table of Greek and Cyrillic capitals that are identical to Latin letters.

THE CONTEXT RULE (callsigns): a zero-like glyph or a homoglyph letter is replaced ONLY inside a whitespace-separated token
that, once replaced, is the shape of a callsign (a prefix of one or two letters, or a digit and a letter; one digit; one to
four letters; an optional /portable or -R suffix), and a zero-like glyph is replaced only where that shape needs a DIGIT (the
digit after the prefix, or a digit of the portable suffix). Anywhere else the glyph is left alone and validation rejects it,
so a place name such as Østerbro is untouched. The ASCII look-alikes O/0, I/l/1, S/5, B/8 and Z/2 are NEVER converted: a
callsign typed with a letter O for the zero is a typo for staff to correct, not something to guess. A page can therefore not
silently make a different callsign out of look-alikes: every homoglyph replacement is listed in `changes`.

MUST NOT import anything from `odoo`. ASCII input is returned as it came, at once, with no per-character work.

Provenance of the tables. Candidates were generated from `unicodedata` names (DIGIT, ZERO, WITH STROKE, BARRED, CROSSED,
SLASH, DOTTED, OVERLAY) and the Latin/Greek/Cyrillic look-alike letters from the Unicode TR39 `confusables.txt` (version
18.0.0, dated 2026-08-06, sha256 6ed3ee967c9dfdf6677d563c9985182fbc50a2efb7d6059cd57b2e2ce18f5b92), read once for
inspection; it is not a runtime dependency and only the small curated result is kept here.
"""
import re
import unicodedata

# Stage 5: precomposed glyphs read as a digit ZERO in a digit position of a callsign (or a number): O/o with stroke,
# empty set, diameter sign, circled division slash, theta and its variants, barred O, Cyrillic barred O and fita, and phi
# (IBM-style slashed zero), as ham text and fonts really use them.
ZERO_LIKE = frozenset(
    "Øø"   # O and o with stroke
    "Ǿǿ"   # O and o with stroke and acute
    "Ꝋꝋ"   # O and o with long stroke overlay
    "∅⌀⊘"   # empty set, diameter sign, circled division slash
    "Θθϴϑ"   # Greek theta (capital, small, lunate capital, script)
    "Ɵɵ"   # O with middle tilde (barred O), barred o
    "ӨөѲѳ"   # Cyrillic barred O, fita
    "Φφϕ"   # Greek phi (capital, small, symbol)
)
# Stage 6: capital (and the lower-case that looks the same) Greek and Cyrillic letters identical to a Latin letter, from
# TR39 confusables. Only ever applied inside a callsign-shaped token, and always reported.
HOMOGLYPH_LETTERS = {
    # Greek capitals
    "Α": "A", "Β": "B", "Ε": "E", "Ζ": "Z", "Η": "H", "Ι": "I", "Κ": "K",
    "Μ": "M", "Ν": "N", "Ο": "O", "Ρ": "P", "Τ": "T", "Υ": "Y", "Χ": "X",
    "Ϲ": "C", "Ϳ": "J",
    # Cyrillic capitals
    "А": "A", "В": "B", "Е": "E", "К": "K", "М": "M", "Н": "H", "О": "O",
    "Р": "P", "С": "C", "Т": "T", "У": "Y", "Х": "X", "Ѕ": "S", "І": "I",
    "Ј": "J",
    # lower-case that looks the same
    "а": "a", "е": "e", "о": "o", "р": "p", "с": "c", "у": "y", "х": "x",
    "ѕ": "s", "і": "i", "ј": "j", "ο": "o",
}
# A letter O with one of these combining overlays is a slashed O, the zero in ham text (stage 4).
_STROKE_OVERLAYS = frozenset(chr(c) for c in range(0x0335, 0x0339))
# Unicode minus, and the dashes and hyphens a page may use for a minus sign.
_MINUS_SIGNS = frozenset("−‐‑‒–—―﹣－⁻₋")
# Arabic decimal and thousands separators, and the separators that NFKC leaves as they are.
_SEPARATORS = {"٫": ".", "٬": ",", "․": ".", "،": ",", "‚": ","}

_CALLSIGN_SHAPE = re.compile(
    r"^(?P<pre>[A-Z]{1,2}|[0-9][A-Z]|[A-Z][0-9])(?P<d>[0-9])(?P<suf>[A-Z]{1,4})(?:[/-](?P<port>[A-Z0-9]{1,4}))?$"
)
_EDGE_PUNCTUATION = ".,;:()[]\"'"
_NUMBER_SHAPE = re.compile(r"^[+-]?[0-9]+(?:[.,][0-9]+)*$")


# [@ANCHOR: zero_sudo:glyph_normalizer_code]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
def _code(text):
    return " ".join(f"U+{ord(ch):04X}" for ch in text)


# [@ANCHOR: zero_sudo:glyph_normalizer_nfkc]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
def _nfkc(ch):
    return unicodedata.normalize("NFKC", ch)


# [@ANCHOR: zero_sudo:glyph_normalizer_digit_of]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
def _digit_of(ch):
    """The ASCII digit a non-ASCII character stands for, or None: any Nd character through unicodedata.decimal, any other
    digit-valued character (a No such as a dingbat circled digit) through unicodedata.digit."""
    category = unicodedata.category(ch)
    try:
        if category == "Nd":
            return str(unicodedata.decimal(ch))
        if category == "No":
            return str(unicodedata.digit(ch))
    except ValueError:
        return None
    return None


# [@ANCHOR: zero_sudo:glyph_normalizer_clean_units]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
def _clean_units(text, changes):
    """Stages 1 to 4. Returns a list of units [text, original, zero_like] where `original` is the glyph(s) the unit came from."""
    units = []
    for ch in text:
        if ch < "\x80":
            units.append([ch, ch, False])
            continue
        category = unicodedata.category(ch)
        if category == "Cf":
            changes.append((ch, _code(ch), ""))
            continue
        if category in ("Zs", "Zl", "Zp"):
            changes.append((ch, _code(ch), " "))
            units.append([" ", ch, False])
            continue
        if category in ("Mn", "Me") and units:
            base = units[-1]
            if base[0] in "0123456789":
                changes.append((ch, _code(ch), ""))  # the stroke of a crossed seven, the dot of a dotted zero
                continue
            if base[0] in "Oo" and not base[2] and ch in _STROKE_OVERLAYS:
                changes.append((base[1] + ch, _code(base[1] + ch), "Ø"))
                units[-1] = ["Ø", base[1] + ch, True]
                continue
            units.append([ch, ch, False])
            continue
        flat = _nfkc(ch)
        if flat != ch and flat:
            if len(flat) == 1 and flat < "\x80":
                changes.append((ch, _code(ch), flat))
                units.append([flat, ch, False])
                continue
            # a ligature or a compatibility sequence: keep what NFKC makes of it, unit by unit
            changes.append((ch, _code(ch), flat))
            for piece in flat:
                units.append([piece, ch, False])
            continue
        digit = _digit_of(ch)
        if digit is not None:
            changes.append((ch, _code(ch), digit))
            units.append([digit, ch, False])
            continue
        units.append([ch, ch, ch in ZERO_LIKE])
    return units


# [@ANCHOR: zero_sudo:glyph_normalizer_callsign_candidate]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
def _callsign_candidate(token_units):
    """(text, zero_like_indexes, homoglyph_indexes) of a token with its zero-like glyphs read as 0 and its homoglyph
    letters as the Latin letter."""
    chars, zeros, homoglyphs = [], [], []
    for index, (shown, _original, zero) in enumerate(token_units):
        if zero or shown in ZERO_LIKE:
            chars.append("0")
            zeros.append(index)
        elif shown in HOMOGLYPH_LETTERS:
            chars.append(HOMOGLYPH_LETTERS[shown])
            homoglyphs.append(index)
        else:
            chars.append(shown)
    return "".join(chars), zeros, homoglyphs


# [@ANCHOR: zero_sudo:glyph_normalizer_digit_positions]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
def _digit_positions(match):
    spans = [match.span("d")]
    if match.group("port") is not None:
        spans.append(match.span("port"))
    return {i for start, end in spans for i in range(start, end)}


# [@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
def normalize_callsign_text(text):
    """`(clean_text, changes)`: `text` with its digits and spaces repaired (stages 1 to 4) and, inside a token that is then
    the shape of a callsign, its zero-like glyphs read as 0 (only where a digit is required) and its Greek and Cyrillic
    look-alike letters as Latin. See the module docstring. Never raises; a non-string gives ('', [])."""
    if not isinstance(text, str):
        return "", []
    if text.isascii():
        return text, []
    changes = []
    units = _clean_units(text, changes)
    out, token = [], []

    def flush():
        if not token:
            return
        shown = "".join(u[0] for u in token)
        stripped_left = len(shown) - len(shown.lstrip(_EDGE_PUNCTUATION))
        stripped_right = max(stripped_left, len(shown.rstrip(_EDGE_PUNCTUATION)))
        core = token[stripped_left:stripped_right]
        if core and any(u[2] or u[0] in ZERO_LIKE or u[0] in HOMOGLYPH_LETTERS for u in core):
            candidate, zeros, homoglyphs = _callsign_candidate(core)
            letters = [ch for ch in candidate if ch.isalpha()]
            one_case = all(ch.isupper() for ch in letters) or all(ch.islower() for ch in letters)
            match = _CALLSIGN_SHAPE.match(candidate.upper()) if one_case else None  # "Bj\u00f8rn" is a word, not a callsign
            if match and set(zeros) <= _digit_positions(match):
                for index in zeros + homoglyphs:
                    original = core[index][1]
                    changes.append((original, _code(original), candidate[index]))
                    core[index][0] = candidate[index]
        out.extend(u[0] for u in token[:stripped_left])
        out.extend(u[0] for u in core)
        out.extend(u[0] for u in token[stripped_right:])
        token.clear()

    for unit in units:
        if unit[0] == " ":
            flush()
            out.append(" ")
        else:
            token.append(unit)
    flush()
    return "".join(out), changes


# [@ANCHOR: zero_sudo:glyph_normalizer_normalize_number_text]
# Verified by [@ANCHOR: zero_sudo:test_glyph_normalizer_number]
def normalize_number_text(text):
    """`(clean_text, changes)`: `text` with digits of any script, full-width and Arabic separators and the Unicode minus
    sign turned into ASCII (stages 1 to 4), a zero-like glyph read as 0 inside a token that is otherwise a number
    (14Ø.5 is 140.5), and the Arabic decimal and thousands separators as '.' and ','. Separators are not
    interpreted: '146,730' stays as written. Never raises; a non-string gives ('', [])."""
    if not isinstance(text, str):
        return "", []
    if text.isascii():
        return text, []
    changes = []
    units = _clean_units(text, changes)
    for index, unit in enumerate(units):
        shown = unit[0]
        if shown in _SEPARATORS:
            changes.append((unit[1], _code(unit[1]), _SEPARATORS[shown]))
            unit[0] = _SEPARATORS[shown]
        elif shown in _MINUS_SIGNS and index + 1 < len(units) and units[index + 1][0].isdigit():
            changes.append((unit[1], _code(unit[1]), "-"))
            unit[0] = "-"
    out, token = [], []

    def flush():
        if not token:
            return
        if any(u[2] or u[0] in ZERO_LIKE for u in token):
            candidate = "".join("0" if (u[2] or u[0] in ZERO_LIKE) else u[0] for u in token)
            if _NUMBER_SHAPE.match(candidate) and sum(ch.isdigit() for ch in candidate) > sum(
                    1 for u in token if u[2] or u[0] in ZERO_LIKE):
                for u in token:
                    if u[2] or u[0] in ZERO_LIKE:
                        changes.append((u[1], _code(u[1]), "0"))
                        u[0] = "0"
        out.extend(u[0] for u in token)
        token.clear()

    for unit in units:
        if unit[0] == " ":
            flush()
            out.append(" ")
        else:
            token.append(unit)
    flush()
    return "".join(out), changes
