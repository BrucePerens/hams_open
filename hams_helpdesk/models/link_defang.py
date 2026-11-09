# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Defanged links for tickets whose triage verdict is phishing (Bruce, 2026-11-08).

The shared text filter (untrusted_text.py) keeps ``<a href>`` on purpose and its tests assert it, so a link in the stored
text of a phishing ticket is live. Staff reading such a ticket must not be one click from the phish. This module turns the
links of a text into plain text at display time (the ticket form, the chatter and the notification email), so the stored
text is untouched and a person's correction (moving the ticket out of Spam) shows the links again. The full description of
every function is in README.md ("Phishing links are defanged for staff").
"""

import html as _html
import re

from lxml import etree
from lxml import html as lxml_html
from markupsafe import Markup

# [@ANCHOR: hams_helpdesk:defang_warning_text]
# The warning the staff form and the notification email show for a ticket whose links are defanged. Written to the ADR-0112
# wording rules: it states what is shown and what to do, and promises nothing about privacy.
DEFANG_WARNING_TEXT = (
    "Phishing warning: the triage verdict for this ticket is phishing. The links in its text are shown as plain text and "
    "cannot be clicked: hxxp and hxxps stand for http and https, and [.] stands for a dot. Do not open them. If you decide "
    "the ticket is genuine, move it out of the Spam stage and its links work again."
)

# The attributes that make an element fetch or follow a URL. On a link they are replaced by plain text; on any other element
# (an image, a form, a frame) they are removed.
_URL_ATTRIBUTES = ("href", "src", "action", "formaction", "background", "poster", "srcset", "data", "cite", "ping", "xlink:href")
_URL_IN_TEXT = re.compile(r"(?i)\b(?:(?:https?|ftp)://|mailto:|www\.)[^\s<>\"']+")
_CLASS_NAME = "o_link_defanged"


# [@ANCHOR: hams_helpdesk:defang_url_text]
def defang_url_text(url):
    """Returns ``url`` as inert plain text: the scheme ``http`` becomes ``hxxp`` (so ``https`` becomes ``hxxps``) and ``ftp``
    becomes ``fxp``, every dot becomes ``[.]`` and every ``@`` becomes ``[@]``. The result matches no link detector, in a
    browser or in Odoo's chatter. Text that is not a URL is defanged the same way, which is the point: the caller never has
    to decide whether it is one."""
    text = str(url or "").strip()
    text = re.sub(r"(?i)^h(tt)(ps?):", lambda m: "hxx" + m.group(2).lower() + ":", text)
    text = re.sub(r"(?i)^ftp:", "fxp:", text)
    return text.replace(".", "[.]").replace("@", "[@]")


# [@ANCHOR: hams_helpdesk:defang_text_node]
def defang_text_node(text):
    """Returns ``text`` with every URL written out in it (``http://``, ``https://``, ``ftp://``, ``mailto:``, ``www.``) defanged by
    ``defang_url_text``; a falsy ``text`` comes back unchanged. Plain text in, plain text out (no markup is read or written)."""
    return _URL_IN_TEXT.sub(lambda m: defang_url_text(m.group(0)), text) if text else text


# [@ANCHOR: hams_helpdesk:defang_html]
def defang_html(value):
    """Returns ``value`` (an HTML fragment or plain text) with no live link in it, as ``Markup``. A URL written out in the text
    is defanged first, so nothing is left for a reader's software to link. Then each ``<a>`` becomes a
    ``<span class="o_link_defanged">`` holding the link's own text and, in brackets, the defanged address (the address is left
    out when the visible text already shows it); every other element loses its URL-bearing attributes (``src``, ``action``
    and the rest). A falsy ``value`` comes back empty. If the fragment cannot be parsed, the whole text is escaped and shown
    as text, which has no link either. It never raises for any string. It does not change the stored text and costs one parse
    of the fragment."""
    if not value:
        return Markup("")
    raw = str(value)
    try:
        wrapper = lxml_html.fragment_fromstring(raw, create_parent="div")
    except (etree.LxmlError, ValueError):
        return Markup(_html.escape(defang_text_node(raw), quote=False))
    # A comment or processing instruction is not shown, but the text after it is: dropping it joins the text on both sides, so a
    # URL written out after one is defanged like any other.
    etree.strip_tags(wrapper, etree.Comment, etree.ProcessingInstruction)
    elements = [e for e in wrapper.iter() if isinstance(e.tag, str)]
    for element in elements:
        element.text = defang_text_node(element.text)
        if element is not wrapper:
            element.tail = defang_text_node(element.tail)
    for element in elements:
        if element is wrapper:
            continue
        if element.tag in ("a", "area"):
            address = defang_url_text(element.get("href") or "")
            visible = "".join(element.itertext())
            element.attrib.clear()
            element.tag = "span"
            element.set("class", _CLASS_NAME)
            if address and address not in visible:
                note = " (%s)" % address
                if len(element):
                    element[-1].tail = (element[-1].tail or "") + note
                else:
                    element.text = (element.text or "") + note
        else:
            for name in _URL_ATTRIBUTES:
                if name in element.attrib:
                    del element.attrib[name]
    parts = [_html.escape(wrapper.text or "", quote=False)]
    parts.extend(etree.tostring(child, encoding="unicode", method="html") for child in wrapper)
    return Markup("".join(parts))
