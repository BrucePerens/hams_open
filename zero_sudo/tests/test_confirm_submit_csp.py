# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

import os
import re
import unittest

from odoo.tests.common import tagged

from .common import HamsTransactionCase

_REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


@tagged("post_install", "-at_install")
class TestConfirmSubmitShipsInFrontendBundle(HamsTransactionCase):
    def test_confirm_submit_js_is_in_web_assets_frontend(self):
        # Tests [@ANCHOR: zero_sudo:confirm_submit_delegated_listener]
        """The delegated `data-hams-confirm` listener only protects the erasure/lockout forms if
        it is actually loaded on public pages -- resolve web.assets_frontend through ir.asset,
        the same resolution the real page render uses."""
        paths = [path for _addon, path, *_rest in self.env["ir.asset"]._get_asset_paths("web.assets_frontend", {})]
        self.assertTrue(
            any(path.endswith("zero_sudo/static/src/js/confirm_submit.js") for path in paths),
            "zero_sudo/static/src/js/confirm_submit.js is missing from web.assets_frontend",
        )


class TestNoInlineEventHandlerAttributesInHamsOpen(unittest.TestCase):
    """Static regression scan: no template this repo ships may carry an inline event-handler
    attribute (onclick=, onsubmit=, ...). Such an attribute runs only under a script-src that
    allows 'unsafe-inline'; a CSP nonce never authorizes it
    (https://www.w3.org/TR/CSP3/#grammardef-nonce-source). hams_com's content_security_policy
    drops 'unsafe-inline' on every response, so an inline handler there is silently dead -- for
    the erasure and lockout forms that meant an irreversible submit with no confirmation.

    Covers server-rendered views (any case) and OWL markup under static/src (lowercase only: a
    literal lowercase `onclick="..."` in an OWL template is rendered as a real HTML attribute and
    is just as dead, while camelCase `onClose=`/`onQsy=` are component props, not handlers)."""

    _VIEW_RE = re.compile(
        r"(?<![\w.:-])(?:t-att(?:f)?-)?on(?:click|load|error|mouseover|mouseout|mousedown|mouseup|"
        r"change|submit|focus|blur|keyup|keydown|keypress|dblclick|input|drag|drop|reset|select)"
        r"\s*=\s*[\"']",
        re.IGNORECASE,
    )
    _OWL_RE = re.compile(_VIEW_RE.pattern)  # same pattern, case-sensitive
    _SKIP_DIRS = {".git", ".claude", "node_modules", "lib", "target", "tests", "hams_shared", "i18n"}

    def test_no_shipped_template_carries_an_inline_event_handler_attribute(self):
        # Tests [@ANCHOR: zero_sudo:confirm_submit_delegated_listener]
        violations = []
        for dirpath, dirnames, filenames in os.walk(_REPO_ROOT):
            dirnames[:] = [d for d in dirnames if d not in self._SKIP_DIRS]
            norm = dirpath.replace(os.sep, "/")
            in_static = "/static/" in norm or norm.endswith("/static")
            if in_static and "/static/src" not in norm:
                continue
            pattern = self._OWL_RE if in_static else self._VIEW_RE
            for filename in filenames:
                if not filename.endswith(".xml"):
                    continue
                filepath = os.path.join(dirpath, filename)
                with open(filepath, "r", encoding="utf-8") as f:
                    content = f.read()
                for match in pattern.finditer(content):
                    lineno = content[: match.start()].count("\n") + 1
                    violations.append(f"{os.path.relpath(filepath, _REPO_ROOT)}:{lineno}")
        self.assertEqual(
            violations,
            [],
            "Inline event-handler attribute(s) found; they never run under a script-src without "
            "'unsafe-inline'. Use data-hams-confirm (zero_sudo/static/src/js/confirm_submit.js) or "
            "an OWL t-on-* handler instead: " + ", ".join(violations),
        )

    def test_destructive_account_forms_use_data_hams_confirm(self):
        # Tests [@ANCHOR: zero_sudo:confirm_submit_delegated_listener]
        for relpath, action in (
            ("user_websites/views/user_websites_templates.xml", "/my/privacy/delete_content"),
            ("hams_base/views/unsubscribe_templates.xml", "/unsubscribe/lockout"),
        ):
            filepath = os.path.join(_REPO_ROOT, relpath)
            if not os.path.exists(filepath):
                continue  # module not present in this checkout
            with open(filepath, "r", encoding="utf-8") as f:
                content = f.read()
            form_tag = re.search(r'<form action="%s"[^>]*>' % re.escape(action), content)
            self.assertIsNotNone(form_tag, f"{relpath}: form {action} not found")
            self.assertIn("data-hams-confirm=", form_tag.group(0), f"{relpath}: {action} lost its confirmation")
