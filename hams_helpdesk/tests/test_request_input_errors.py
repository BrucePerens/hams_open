# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Malformed input on a public page route is a 404, not a 500 and a ticket (Bruce, 2026-10-19)."""

import logging

import psycopg2.errors
import werkzeug.exceptions

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase

from odoo.addons.hams_helpdesk.models.exception_scrub import is_expected
from odoo.addons.hams_helpdesk.models.ir_http_exception import (
    _error_page_failed_on_the_url,
    _input_errors_are_the_callers,
)
from odoo.addons.hams_helpdesk.models.request_input_errors import (
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
