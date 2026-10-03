# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shared SVG fixtures for the SVG allowlist sanitizer's tests.

`SCHEMATIC_SVG` is a representative, safe schematic (resistor, opto-isolator,
relay coil and contact, labels). `XSS_CORPUS` is a list of
(name, markup) pairs of known SVG/HTML XSS and mutation-XSS vectors. Every
script payload sets `window.__xss` (never `alert`, which would hang a headless
browser), so the browser test can prove that nothing ran.
"""

# Every payload below does the same observable thing if it ever runs.
_X = "window.__xss=1"
_EVIL = "https://evil.example/hams-svg-probe"
# Looks like the sanitizer's own placeholder; it must stay plain text.
FORGED_TOKEN = "hamssvg0123456789abcdef01234567x0x"

SCHEMATIC_SVG = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 220"
     width="100%" height="auto" role="img"
     aria-label="Relay driven by an opto-isolator"
     style="font-family: sans-serif; background-color: #ffffff; margin-bottom: 20px;">
  <title>Relay driven by an opto-isolator</title>
  <defs>
    <marker id="dot" viewBox="0 0 10 10" refX="5" refY="5" markerWidth="6" markerHeight="6">
      <circle cx="5" cy="5" r="3" fill="#000"/>
    </marker>
    <linearGradient id="coilfill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#eef"/>
      <stop offset="100%" stop-color="#99c"/>
    </linearGradient>
    <clipPath id="clipbox"><rect x="0" y="0" width="400" height="220"/></clipPath>
  </defs>
  <g clip-path="url(#clipbox)" stroke="currentColor" fill="none" stroke-width="2">
    <!-- 1k resistor -->
    <rect id="r1" x="40" y="40" width="60" height="20"/>
    <line x1="10" y1="50" x2="40" y2="50" marker-start="url(#dot)"/>
    <line x1="100" y1="50" x2="140" y2="50"/>
    <text x="70" y="35" text-anchor="middle" fill="currentColor" stroke="none" font-size="12">1k&#937;</text>
    <!-- opto-isolator -->
    <rect x="140" y="20" width="90" height="100" fill="#f9f9f9"/>
    <text x="185" y="40" text-anchor="middle" font-weight="bold" fill="currentColor" stroke="none">4N25</text>
    <polygon points="160,60 160,90 180,75" fill="currentColor"/>
    <polyline points="190,60 200,70 195,75 210,85" stroke-dasharray="4 2" stroke-linecap="round"/>
    <!-- relay coil and contact -->
    <ellipse cx="300" cy="60" rx="30" ry="14" fill="url(#coilfill)"/>
    <path d="M 270 120 C 280 160, 320 160, 330 120" stroke-linejoin="round" marker-end="url(#dot)"/>
    <circle cx="300" cy="180" r="6" fill="currentColor" fill-opacity="0.8"/>
    <text x="300" y="205" text-anchor="middle" fill="currentColor" stroke="none">K1 <tspan font-style="italic">coil</tspan></text>
  </g>
</svg>"""

XSS_CORPUS = [
    ("script_element", f"<svg><script>{_X}</script></svg>"),
    ("script_element_cdata", f"<svg><script><![CDATA[{_X}]]></script></svg>"),
    ("onload_on_svg", f"<svg onload={_X}><rect width=5 height=5 /></svg>"),
    ("onclick_on_rect", f"<svg><rect onclick='{_X}' width='5' height='5'/></svg>"),
    ("onerror_on_image", f"<svg><image href='x' onerror='{_X}'/></svg>"),
    ("onmouseover_on_text", f"<svg><text onmouseover='{_X}'>hover</text></svg>"),
    ("onbegin_on_animate", f"<svg><animate onbegin='{_X}' attributeName='x' dur='1s'/></svg>"),
    ("onfocus_autofocus", f"<svg><a tabindex=1 autofocus onfocus='{_X}'><text>f</text></a></svg>"),
    ("a_href_javascript", f"<svg><a href='javascript:{_X}'><text>click</text></a></svg>"),
    ("a_xlink_href_javascript", f"<svg xmlns:xlink='http://www.w3.org/1999/xlink'><a xlink:href='javascript:{_X}'><text>click</text></a></svg>"),
    ("animate_href_javascript", f"<svg><a><animate attributeName='href' values='javascript:{_X}'/><text>click</text></a></svg>"),
    ("set_href_javascript", f"<svg><a><set attributeName='href' to='javascript:{_X}'/><text>click</text></a></svg>"),
    ("set_event_handler", f"<svg><rect width=9 height=9><set attributeName='onmouseover' to='{_X}'/></rect></svg>"),
    ("animatetransform_onend", f"<svg><rect><animateTransform attributeName='transform' type='rotate' onend='{_X}' dur='1s'/></rect></svg>"),
    ("animatemotion", f"<svg><rect><animateMotion onbegin='{_X}' path='M0 0'/></rect></svg>"),
    ("use_data_uri", "<svg><use href=\"data:image/svg+xml;base64,PHN2Zz48c2NyaXB0PndpbmRvdy5fX3hzcz0xPC9zY3JpcHQ+PC9zdmc+#x\"/></svg>"),
    ("use_data_uri_xlink", "<svg><use xlink:href=\"data:image/svg+xml,&lt;svg id='x' xmlns='http://www.w3.org/2000/svg'&gt;&lt;script&gt;window.__xss=1&lt;/script&gt;&lt;/svg&gt;#x\"/></svg>"),
    ("use_external", f"<svg><use xlink:href='{_EVIL}.svg#a'/></svg>"),
    ("use_local_fragment", "<svg><defs><g id='a'><script>window.__xss=1</script></g></defs><use href='#a'/></svg>"),
    ("image_href_javascript", f"<svg><image href='javascript:{_X}'/></svg>"),
    ("image_external", f"<svg><image href='{_EVIL}.png' width='5' height='5'/></svg>"),
    ("foreignobject_script", f"<svg><foreignObject><script>{_X}</script></foreignObject></svg>"),
    ("foreignobject_iframe", f"<svg><foreignObject><iframe src='{_EVIL}'></iframe></foreignObject></svg>"),
    ("foreignobject_body_onload", f"<svg><foreignObject><body onload='{_X}'></foreignObject></svg>"),
    ("foreignobject_img_onerror", f"<svg><foreignObject><img src=x onerror='{_X}'></foreignObject></svg>"),
    ("style_element_import", f"<svg><style>@import url({_EVIL}.css);</style><rect/></svg>"),
    ("style_element_url", f"<svg><style>rect{{fill:url({_EVIL}#a)}}</style><rect/></svg>"),
    ("style_element_expression", "<svg><style>rect{width:expression(window.__xss=1)}</style></svg>"),
    ("style_attr_url_external", f"<svg><rect style='fill:url({_EVIL}#a)' width=5 height=5/></svg>"),
    ("style_attr_javascript", f"<svg><rect style='background:url(javascript:{_X})' width=5 height=5/></svg>"),
    ("style_attr_expression", "<svg><rect style='width:expression(window.__xss=1)' width=5 height=5/></svg>"),
    ("style_attr_import", f"<svg><rect style='@import \"{_EVIL}\"' width=5 height=5/></svg>"),
    ("style_attr_css_escape", "<svg><rect style='fill:\\75 rl(https://evil.example/x)' width=5 height=5/></svg>"),
    ("style_attr_comment_smuggle", f"<svg><rect style='fill:u/**/rl({_EVIL})' width=5 height=5/></svg>"),
    ("entity_external_file", "<!DOCTYPE svg [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><svg><text>&xxe;</text></svg>"),
    ("entity_external_http", f"<!DOCTYPE svg [<!ENTITY xxe SYSTEM \"{_EVIL}\">]><svg><text>&xxe;</text></svg>"),
    ("entity_billion_laughs", "<!DOCTYPE svg [<!ENTITY a \"aaaaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\"><!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;\"><!ENTITY d \"&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;\">]><svg><text>&d;&d;&d;&d;</text></svg>"),
    ("entity_parameter", f"<!DOCTYPE svg [<!ENTITY % p SYSTEM \"{_EVIL}.dtd\"> %p;]><svg><text>x</text></svg>"),
    ("entity_external_dtd", f"<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"{_EVIL}.dtd\"><svg><text>x</text></svg>"),
    ("entity_internal_script", "<!DOCTYPE svg [<!ENTITY s \"&lt;script&gt;window.__xss=1&lt;/script&gt;\">]><svg><text>&s;</text></svg>"),
    ("mxss_svg_style_title", f"<svg></p><style><a id=\"</style><img src=1 onerror={_X}>\">"),
    ("mxss_svg_style_img", f"<svg><style><img src=x onerror='{_X}'></style></svg>"),
    ("mxss_svg_title_style", f"<svg><title><style><img src=x onerror='{_X}'></style></title></svg>"),
    ("mxss_math_mglyph_style", f"<math><mtext><table><mglyph><style><!--</style><img title=\"--&gt;&lt;/mglyph&gt;&lt;img&Tab;src=1&Tab;onerror={_X}&gt;\">"),
    ("mxss_noscript_attr", f"<noscript><p title=\"</noscript><img src=x onerror={_X}>\"></noscript><svg><rect/></svg>"),
    ("mxss_textarea_svg", f"<textarea><svg><script>{_X}</script></svg></textarea><svg><rect/></svg>"),
    ("mxss_title_svg", f"<title><svg onload={_X}></title><svg><rect/></svg>"),
    ("mxss_desc_cdata", f"<svg><desc><![CDATA[</desc><script>{_X}</script>]]></desc></svg>"),
    ("mxss_nested_svg_break_out", f"<svg><svg onload={_X}><p><rect/></p></svg></svg>"),
    ("mxss_br_breaks_foreign", f"<svg><desc><br><img src=x onerror={_X}></desc></svg>"),
    ("mxss_listing_pre", f"<svg><pre><img src=x onerror={_X}></pre></svg>"),
    ("mxss_xmp", f"<xmp><svg><script>{_X}</script></svg></xmp>"),
    ("mxss_plaintext", f"<plaintext><svg onload={_X}>"),
    ("mxss_iframe_srcdoc", f"<iframe srcdoc='&lt;svg onload={_X}&gt;'></iframe><svg><rect/></svg>"),
    ("case_mixed_tag_and_handler", f"<SvG OnLoAd={_X}><ScRiPt>{_X}</sCrIpT></SvG>"),
    ("whitespace_slash_handler", f"<svg\n/onload={_X}>"),
    ("whitespace_tab_in_scheme", "<svg><a href=\"jav&#x09;ascript:window.__xss=1\"><text>a</text></a></svg>"),
    ("whitespace_newline_in_scheme", "<svg><a href=\"java\nscript:window.__xss=1\"><text>a</text></a></svg>"),
    ("entity_encoded_scheme", "<svg><a href=\"&#106;avascript&colon;window.__xss=1\"><text>a</text></a></svg>"),
    ("hex_entity_scheme", "<svg><a href=\"&#x6A;&#x61;vascript:window.__xss=1\"><text>a</text></a></svg>"),
    ("null_byte_in_tag", f"<svg><scr\x00ipt>{_X}</scr\x00ipt></svg>"),
    ("null_byte_in_attr", f"<svg onload\x00={_X}><rect/></svg>"),
    ("null_byte_in_scheme", "<svg><a href='jav\x00ascript:window.__xss=1'><text>a</text></a></svg>"),
    ("attr_injection_double_quote", f"<svg><rect fill='red\" onload=\"{_X}' width=1 height=1 /></svg>"),
    ("attr_injection_id", f"<svg><rect id='a\" onmouseover=\"{_X}' width=1 height=1 /></svg>"),
    ("attr_injection_class_entity", f"<svg><rect class=\"a&quot; onload=&quot;{_X}\" width=1 height=1 /></svg>"),
    ("attr_injection_backtick", f"<svg><rect class=`a onload={_X}` /></svg>"),
    ("attr_injection_aria", f"<svg aria-label=\"x\" onload=\"{_X}\" role=\"img\"><rect/></svg>"),
    ("attr_injection_d", f"<svg><path d='M0 0 L1 1\" onload=\"{_X}' /></svg>"),
    ("comment_svg", f"<!-- <svg onload={_X}><script>{_X}</script></svg> --><p>c</p>"),
    ("comment_break_out", f"<!-- x --!><svg onload={_X}><rect/></svg>"),
    ("conditional_comment", f"<!--[if IE]><svg onload={_X}><![endif]--><p>c</p>"),
    ("svg_inside_attribute", f"<p title='<svg onload={_X}><rect/></svg>'>t</p>"),
    ("svg_inside_attribute_script", f"<p title=\"<svg><script>{_X}</script></svg>\" class=a>t</p>"),
    ("svg_inside_data_attribute", f"<div data-x='<svg onload={_X}>'><svg><rect/></svg></div>"),
    ("cdata_script", f"<svg><![CDATA[<script>{_X}</script>]]></svg>"),
    ("cdata_in_text_break_out", f"<svg><text><![CDATA[</text><script>{_X}</script>]]></text></svg>"),
    ("namespace_prefix_script", f"<svg:script>{_X}</svg:script>"),
    ("namespace_prefix_use", "<svg:use href='data:image/svg+xml,x'/>"),
    ("namespace_prefix_svg", f"<svg:svg onload={_X}><svg:rect/></svg:svg>"),
    ("namespace_declared_prefix", f"<svg xmlns:x='http://www.w3.org/1999/xhtml'><x:script>{_X}</x:script></svg>"),
    ("namespace_xmlns_override", f"<svg xmlns='http://www.w3.org/1999/xhtml'><script>{_X}</script></svg>"),
    ("namespace_xmlns_javascript", f"<svg xmlns='javascript:{_X}'><rect/></svg>"),
    ("filter_feimage", f"<svg><filter id='f'><feImage href='{_EVIL}.png'/></filter><rect filter='url(#f)' width=5 height=5 /></svg>"),
    ("filter_fe_others", "<svg><filter id='f'><feGaussianBlur stdDeviation='2'/><feOffset dx='2'/></filter><rect filter='url(#f)' width=5 height=5 /></svg>"),
    ("mask_element", "<svg><mask id='m'><rect width=5 height=5 /></mask><rect mask='url(#m)' width=5 height=5 /></svg>"),
    ("switch_element", f"<svg><switch><foreignObject><script>{_X}</script></foreignObject><text>t</text></switch></svg>"),
    ("symbol_use", f"<svg><symbol id='s'><script>{_X}</script></symbol><use href='#s'/></svg>"),
    ("iframe_in_svg", f"<svg><iframe src='javascript:{_X}'></iframe></svg>"),
    ("object_in_svg", f"<svg><object data='javascript:{_X}'></object></svg>"),
    ("embed_in_svg", f"<svg><embed src='javascript:{_X}'></svg>"),
    ("link_in_svg", f"<svg><link rel=stylesheet href='{_EVIL}.css'></svg>"),
    ("meta_refresh_in_svg", f"<svg><meta http-equiv=refresh content='0;url=javascript:{_X}'></svg>"),
    ("base_in_svg", f"<svg><base href='{_EVIL}/'><rect/></svg>"),
    ("paint_external_url", f"<svg><rect fill='url({_EVIL}#a)' width=5 height=5 /></svg>"),
    ("paint_javascript_url", f"<svg><rect fill=\"url('javascript:{_X}')\" width=5 height=5 /></svg>"),
    ("clip_path_data_url", "<svg><rect clip-path='url(data:image/svg+xml,x#c)' width=5 height=5 /></svg>"),
    ("marker_protocol_relative", "<svg><line marker-end='url(//evil.example/x#m)' x2=5 /></svg>"),
    ("stroke_url_fallback_external", f"<svg><rect stroke='url(#ok) url({_EVIL})' width=5 height=5 /></svg>"),
    ("text_content_is_markup", f"<svg><text>&lt;img src=x onerror={_X}&gt;</text></svg>"),
    ("text_content_closing_tags", f"<svg><text></text></svg></div></body></html><img src=x onerror={_X}>"),
    ("forged_token", f"<p>{FORGED_TOKEN}</p><svg><rect/></svg>"),
    ("unclosed_svg", f"<svg><rect onclick={_X}>"),
    ("misnested_svg", f"<div><svg><circle r=5 onclick={_X} /></div></svg>"),
    ("svg_in_p_in_a", f"<a href='javascript:{_X}'><p><svg><rect/></svg></p></a>"),
    ("tspan_x_list_injection", f"<svg><text dx='1,2\" onload=\"{_X}'>t</text></svg>"),
    ("textpath_href", "<svg><text><textPath href='data:text/plain,x'>t</textPath></text></svg>"),
    ("data_attribute", f"<svg><rect data-x='{_X}' data-bind='javascript:1' width=5 height=5 /></svg>"),
    ("sandbox_escape_srcdoc", f"<svg><foreignObject><iframe srcdoc='&lt;script&gt;{_X}&lt;/script&gt;'></iframe></foreignObject></svg>"),
]

# Everything the allowlist permits; anything else in cleaned output is a failure.
SAFE_ELEMENTS = frozenset(
    "svg g path rect circle ellipse line polyline polygon text tspan defs "
    "title desc linearGradient radialGradient stop clipPath marker symbol "
    "pattern".split()
)
