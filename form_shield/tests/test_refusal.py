# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, SafePatchMixin
from unittest import mock

from odoo.tests import BaseCase, tagged

from .test_support import reset_shield_state, shield_fields
from ..models import form_shield as shield_module
from .. import refusal
from ..refusal import PAGE_STATUS, _with_query

BAD_ANSWER = "v1.pow.1.2.3.aa.bb.cc~1"


@tagged("post_install", "-at_install", "form_shield")
class TestRefusalResponse(HamsHttpCase):
    """refusal_response() as the hams_com report forms use it, through a route that is not one of
    them: Odoo's own /website/form is covered in test_http, the report pages in their modules."""

    def setUp(self):
        super().setUp()
        reset_shield_state(self.env)

    # [@ANCHOR: form_shield:test_refusal_response_by_outcome]
    def test_refusal_response_by_outcome(self):
        # Tests [@ANCHOR: form_shield:with_query]
        # Tests [@ANCHOR: form_shield:refusal_response]
        # Tests [@ANCHOR: form_shield:refused_page]
        self.assertEqual(_with_query("/p", "a=1"), "/p?a=1")
        self.assertEqual(_with_query("/p?x=2", "a=1"), "/p?x=2&a=1")
        self.assertEqual(
            PAGE_STATUS,
            {shield_module.LIMITED: 429, shield_module.UNAVAILABLE: 503},
        )
        self.assertTrue(shield_fields(self.env, "contact"))


@tagged("post_install", "-at_install", "form_shield")
class TestRefusalResponseBranches(SafePatchMixin, BaseCase):
    """Each branch of refusal_response() with a stand-in request: where a refused submission is sent."""

    def _call(self, outcome, reason="why", question_path=None):
        fake = mock.MagicMock()
        fake.redirect.side_effect = lambda path, local=False: ("redirect", path, local)
        self.safe_patch_object(refusal, "request", fake)
        result = refusal.refusal_response(
            shield_module.Verdict(False, outcome, reason), "/back?x=1", "/ok", question_path
        )
        return result, fake

    def test_silent_goes_to_success_path(self):
        # Tests [@ANCHOR: form_shield:refusal_response]
        result, _fake = self._call(shield_module.SILENT)
        self.assertEqual(result, ("redirect", "/ok", True))

    def test_retry_goes_back_with_reason(self):
        result, _fake = self._call(shield_module.RETRY, "bad_answer")
        self.assertEqual(result, ("redirect", "/back?x=1&form_shield_error=bad_answer", True))

    def test_question_goes_to_question_page_when_there_is_one(self):
        result, _fake = self._call(shield_module.QUESTION, "no_token", "/ask")
        self.assertEqual(result, ("redirect", "/ask", True))

    def test_question_without_page_goes_back_with_reason(self):
        result, _fake = self._call(shield_module.QUESTION, "no_token")
        self.assertEqual(result, ("redirect", "/back?x=1&form_shield_error=no_token", True))

    def test_limited_renders_429_uncached(self):
        result, fake = self._call(shield_module.LIMITED)
        self.assertEqual(result.status_code, 429)
        result.headers.__setitem__.assert_called_with("Cache-Control", "no-store")
        self.assertEqual(fake.render.call_args[0][0], "form_shield.refused_page")

    def test_unavailable_renders_503(self):
        result, _fake = self._call(shield_module.UNAVAILABLE)
        self.assertEqual(result.status_code, 503)
