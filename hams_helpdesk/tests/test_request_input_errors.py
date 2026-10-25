# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Malformed input on a public page route is a 404, not a 500 and a ticket (Bruce, 2026-10-19)."""

import linecache
import logging
import os
import traceback
import types

import psycopg2.errors
import werkzeug.exceptions

from odoo.tests.common import tagged
from odoo.tools import misc as odoo_misc
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase

from odoo.addons.hams_helpdesk.models.exception_scrub import is_expected
from odoo.addons.hams_helpdesk.models.ir_http_exception import (
    _error_page_failed_on_the_url,
    _input_errors_are_the_callers,
)
from odoo.addons.hams_helpdesk.models.request_input_errors import (
    _assert_names,
    classify_request_input_error,
    request_values,
)

_logger = logging.getLogger(__name__)


def _raised(function, *args):
    """The exception ``function`` raises, with its real traceback."""
    try:
        function(*args)
    except Exception as error:  # audit-ignore-catch-all: the test needs the raised object itself
        _logger.info("test handler raised %s", type(error).__name__)
        return error
    raise AssertionError("did not raise")


def _handler_splits(value):
    return value.split(",")  # AttributeError when `value` arrives as a list


def _handler_looks_up(table, key):
    return table[key]  # KeyError quoting the key


def _handler_adds(left, right):
    return left + right  # TypeError, quotes nothing


def _handler_with_a_bug():
    return None.missing  # AttributeError that has nothing to do with the request


@tagged("post_install", "-at_install")
class TestClassifyRequestInputError(HamsTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:classify_request_input_error]
    # Tests [@ANCHOR: hams_helpdesk:input_error_message_quotes_a_value]
    # Tests [@ANCHOR: hams_helpdesk:input_error_callers_object_is_a_local]
    # Tests [@ANCHOR: hams_helpdesk:input_error_inner_frames]

    def test_database_data_errors_are_the_callers_when_the_request_had_parameters(self):
        for error_class in (
            psycopg2.errors.InvalidTextRepresentation,
            psycopg2.errors.NumericValueOutOfRange,
            psycopg2.errors.InvalidDatetimeFormat,
            psycopg2.errors.StringDataRightTruncation,
        ):
            self.assertTrue(classify_request_input_error(error_class("x"), {"id": "99999999999999999999"}))
            self.assertIsNone(classify_request_input_error(error_class("x"), {}), "no input to blame")

    def test_other_database_errors_are_not_converted(self):
        self.assertIsNone(classify_request_input_error(psycopg2.errors.UndefinedTable("x"), {"a": "bcd"}))
        self.assertIsNone(classify_request_input_error(psycopg2.errors.IntegrityError("x"), {"a": "bcd"}))

    def test_a_lookup_that_found_no_record_is_the_callers(self):
        empty = ValueError("Expected singleton: res.users()")
        many = ValueError("Expected singleton: res.users(1, 2)")
        self.assertTrue(classify_request_input_error(empty, {"token": "abcdef"}))
        self.assertIsNone(classify_request_input_error(empty, {}))
        self.assertIsNone(classify_request_input_error(many, {"token": "abcdef"}), "two records is a server bug")

    def test_a_message_that_quotes_a_value_the_caller_sent_is_the_callers(self):
        error = _raised(_handler_looks_up, {}, "zzqq")
        self.assertTrue(classify_request_input_error(error, {"key": "zzqq"}))

    def test_a_message_that_quotes_nothing_the_caller_sent_is_not(self):
        error = _raised(_handler_looks_up, {}, "internal_key")
        self.assertIsNone(classify_request_input_error(error, {"key": "zzqq"}))

    def test_a_short_value_is_not_searched_for_in_messages(self):
        error = ValueError("invalid literal for int() with base 10: 'a'")
        self.assertIsNone(classify_request_input_error(error, {"n": "a"}))

    def test_a_handler_that_failed_on_the_object_the_caller_sent_is_the_callers(self):
        sent = {"tags": ["a", "b"]}
        error = _raised(_handler_splits, sent["tags"])
        self.assertIsInstance(error, AttributeError)
        self.assertTrue(classify_request_input_error(error, sent))

    def test_a_type_error_over_values_not_from_the_request_is_not_converted(self):
        error = _raised(_handler_adds, True, "text")
        self.assertIsInstance(error, TypeError)
        self.assertIsNone(classify_request_input_error(error, {"search": "text-from-caller"}))

    def test_a_server_bug_with_a_parameterized_request_stays_a_500(self):
        error = _raised(_handler_with_a_bug)
        self.assertIsNone(classify_request_input_error(error, {"search": "anything"}))

    def test_classes_outside_the_list_are_never_converted(self):
        for error in (RuntimeError("zzqq"), NameError("zzqq"), PermissionError("zzqq"), ZeroDivisionError("zzqq")):
            self.assertIsNone(classify_request_input_error(error, {"key": "zzqq"}))

    def test_request_values_walks_nested_parameters_and_ignores_objects(self):
        # Tests [@ANCHOR: hams_helpdesk:request_input_values]
        record = self.env.user
        values = request_values({"a": "x1", "b": ["y2", {"c": "z3"}], "rec": record, "n": 5})
        self.assertEqual(sorted(v for v in values if isinstance(v, str)), ["x1", "y2", "z3"])
        self.assertNotIn(record, values)


@tagged("post_install", "-at_install")
class TestInputErrorRouteScope(HamsTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:input_error_route_scope]

    def test_public_page_routes_are_in_scope(self):
        self.assertTrue(_input_errors_are_the_callers({"type": "http", "auth": "public", "methods": ["GET"]}))
        self.assertTrue(_input_errors_are_the_callers({"type": "http", "auth": "none"}))
        self.assertTrue(_input_errors_are_the_callers({"type": "http", "auth": "public", "methods": ["POST"]}))

    def test_login_required_and_json_routes_are_not(self):
        self.assertFalse(_input_errors_are_the_callers({"type": "http", "auth": "user"}))
        self.assertFalse(_input_errors_are_the_callers({"type": "jsonrpc", "auth": "public"}))
        self.assertFalse(_input_errors_are_the_callers({"type": "json", "auth": "public"}))

    def test_a_webhook_style_route_is_not(self):
        self.assertFalse(_input_errors_are_the_callers({"type": "http", "auth": "public", "csrf": False, "methods": ["POST"]}))
        self.assertFalse(_input_errors_are_the_callers({"type": "http", "auth": "public", "csrf": False}))
        self.assertTrue(
            _input_errors_are_the_callers({"type": "http", "auth": "public", "csrf": False, "methods": ["GET"]}),
            "a GET-only route cannot be a webhook",
        )


@tagged("post_install", "-at_install")
class TestUnbuildableErrorPage(HamsHttpCase):
    # Tests [@ANCHOR: hams_helpdesk:render_error_is_the_urls]

    def test_only_the_two_urljoin_refusals_count(self):
        self.assertTrue(_error_page_failed_on_the_url(ValueError("Dot segments are not allowed")))
        self.assertTrue(
            _error_page_failed_on_the_url(
                ValueError("Extra URL must use same scheme and host as base, and begin with base path")
            )
        )
        self.assertFalse(_error_page_failed_on_the_url(ValueError("something else")))
        self.assertFalse(_error_page_failed_on_the_url(TypeError("Dot segments are not allowed")))

    def test_the_paths_scanners_send_are_a_404_and_no_ticket(self):
        # The production 500s of 2026-10-17/18: /.//.env and /@fs/..%252f..%252fproc/self/environ?raw??
        before = self.env["hams_helpdesk.exception.fingerprint"].search_count([])
        for path in ("/.//.env", "/@fs/..%252f..%252f..%252fproc/self/environ?raw??", "/a/%2e%2e/%2e%2e/etc/passwd"):
            response = self.url_open(path, allow_redirects=False)
            self.assertEqual(response.status_code, 404, path)
        self.assertEqual(self.env["hams_helpdesk.exception.fingerprint"].search_count([]), before)

    def test_a_not_found_is_never_a_ticket(self):
        self.assertTrue(is_expected(werkzeug.exceptions.NotFound(), "http", False))


# Where Odoo keeps its controllers. The stand-in handlers below are given a code object whose file name is such a path,
# so they raise from frames that look, to the classifier, exactly like content_common or content_assets in
# odoo/addons/web/controllers/binary.py. linecache serves that name this file's own lines, so an assert's line can be
# read back as it can for the real file.
_FAKE_ODOO_CONTROLLER = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.realpath(odoo_misc.__file__)))),
    "odoo", "addons", "hams_test_fake", "controllers", "binary.py",
)


def _content_common(download=False, **kw):
    download = odoo_misc.str2bool(download)
    return download


def _content_assets(filename=None, assets_params=None, **kw):
    assets_params = assets_params or {}
    assert isinstance(assets_params, dict)
    return filename


def _reads_a_setting(value):
    return odoo_misc.str2bool(value)


def _asserts_on_its_own_value(filename=None, **kw):
    url = "/web/assets/" + (filename or "")
    assert "%" not in url
    return url


def _fake_odoo_controller():
    """The four stand-ins above, as if they lived in an Odoo controller file."""
    own_lines = linecache.getlines(__file__)
    linecache.cache[_FAKE_ODOO_CONTROLLER] = (sum(map(len, own_lines)), None, own_lines, _FAKE_ODOO_CONTROLLER)
    controller = {}
    for function in (_content_common, _content_assets, _reads_a_setting, _asserts_on_its_own_value):
        stand_in = types.FunctionType(
            function.__code__.replace(co_filename=_FAKE_ODOO_CONTROLLER), function.__globals__, function.__name__,
            function.__defaults__,
        )
        stand_in.__kwdefaults__ = function.__kwdefaults__
        controller[function.__name__.lstrip("_")] = stand_in
    return controller


@tagged("post_install", "-at_install")
class TestOdooParameterParsing(HamsTransactionCase):
    """Odoo's own controllers parse some parameters themselves; their refusal of what the caller sent is the caller's.

    Ticket 103 (2026-10-25): /web/content/1297/<file>.pdf?download=514750499 raised
    ValueError("Use 0/1/yes/no/true/false/on/off") in str2bool, called by content_common for ?download=. The message
    quotes nothing, so only the sent value's identity pointed at the caller, and that evidence is absent for an empty or
    one-letter value (CPython shares those string objects)."""

    # Tests [@ANCHOR: hams_helpdesk:input_error_odoo_parameter_parsing]
    # Tests [@ANCHOR: hams_helpdesk:classify_request_input_error]

    def setUp(self):
        super().setUp()
        self.controller = _fake_odoo_controller()

    def _classify(self, function, params):
        return classify_request_input_error(_raised(lambda: function(**params)), params)

    def test_str2bool_refusing_the_download_flag_is_the_callers(self):
        for value in ("bogus", "514750499", "", "x", "2", "TRUE ", "Maybe"):
            params = {"id": 1, "filename": "x.txt", "download": value}
            self.assertTrue(self._classify(self.controller["content_common"], params), repr(value))

    def test_str2bool_refusing_a_value_the_caller_did_not_send_stays_a_500(self):
        error = _raised(self.controller["reads_a_setting"], "enabled-ish")
        self.assertIsNone(classify_request_input_error(error, {"download": "1", "q": "other"}))
        self.assertIsNone(classify_request_input_error(error, {}), "no parameters at all")

    def test_str2bool_called_outside_a_controller_stays_a_500(self):
        # Our own model code reading a setting, with the same text as a parameter by coincidence: not the caller's.
        error = _raised(odoo_misc.str2bool, "")
        self.assertIsNone(classify_request_input_error(error, {"q": ""}))

    def test_a_different_value_error_from_a_controller_stays_a_500(self):
        self.assertIsNone(
            classify_request_input_error(ValueError("Use 0/1/yes/no/true/false/on/off"), {"download": "x"}),
            "the message alone, with no str2bool frame, is not evidence",
        )

    def test_an_odoo_controller_asserting_on_a_parameter_as_sent_is_the_callers(self):
        # /web/assets/<unique>/<filename>?assets_params=ijyM: content_assets asserts the parameter is a dict.
        params = {"unique": "1", "filename": "web.assets_frontend.min.css", "assets_params": "ijyM"}
        error = _raised(lambda: self.controller["content_assets"](**params))
        self.assertIsInstance(error, AssertionError)
        self.assertTrue(classify_request_input_error(error, params))

    def test_an_odoo_controller_asserting_on_its_own_value_stays_a_500(self):
        params = {"filename": "a%b.css"}
        error = _raised(lambda: self.controller["asserts_on_its_own_value"](**params))
        self.assertIsInstance(error, AssertionError)
        self.assertIsNone(classify_request_input_error(error, params), "url is built by the handler, not sent")

    def test_assert_names_reads_only_an_assert_statement(self):
        # Tests [@ANCHOR: hams_helpdesk:input_error_assert_names]
        error = _raised(lambda: self.controller["content_assets"](assets_params="ijyM"))
        frame, line = list(traceback.walk_tb(error.__traceback__))[-1]
        self.assertEqual(_assert_names(frame, line), {"isinstance", "assets_params", "dict"})
        error = _raised(lambda: self.controller["content_common"](download="bogus"))
        frame, line = list(traceback.walk_tb(error.__traceback__))[-2]
        self.assertEqual(_assert_names(frame, line), set(), "not an assert statement")

    def test_an_assertion_in_our_own_code_stays_a_500(self):
        def our_handler(assets_params=None):
            assert isinstance(assets_params, dict)

        params = {"assets_params": "ijyM"}
        self.assertIsNone(classify_request_input_error(_raised(lambda: our_handler(**params)), params))


@tagged("post_install", "-at_install")
class TestOdooBinaryRoutesWithBadParameters(HamsHttpCase):
    """Odoo's /web/content, /web/image and /web/assets routes, called with parameter values they cannot parse, answer a
    4xx with no ticket and nothing logged at error (ticket 103: ?download=514750499 on /web/content was a 500)."""

    # Tests [@ANCHOR: hams_helpdesk:malformed_input_is_404]
    # Tests [@ANCHOR: hams_helpdesk:input_error_odoo_parameter_parsing]

    def _assert_answered(self, url, status):
        fingerprints = self.env["hams_helpdesk.exception.fingerprint"]
        before = fingerprints.search_count([])
        with self.assertNoLogs(level="ERROR"):
            response = self.url_open(url, allow_redirects=False)
        self.assertEqual(response.status_code, status, url)
        self.assertEqual(fingerprints.search_count([]), before, f"{url} filed an exception fingerprint")

    def test_a_download_flag_that_is_not_a_boolean_is_a_404(self):
        for url in (
            "/web/content/1/x.txt?download=bogus",
            "/web/content/1/x.txt?download=514750499",  # the request of ticket 103
            "/web/content/1/x.txt?download=",
            "/web/content/1/x.txt?download=x",
            "/web/content?download=2",
            "/web/image/1?download=x",
            "/web/image/1/x.png?download=",
        ):
            self._assert_answered(url, 404)

    def test_asset_parameters_that_are_not_a_dict_are_a_404(self):
        self._assert_answered("/web/assets/1/web.assets_frontend.min.css?assets_params=ijyM", 404)

    def test_an_image_size_that_is_not_a_number_stays_main_s_400(self):
        # int("abc") in content_image: the existing not-a-number rule, a 400 logged at warning.
        self._assert_answered("/web/image/1?width=abc", 400)
        self._assert_answered("/web/image/1?height=", 400)
