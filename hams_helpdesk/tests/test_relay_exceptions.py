# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Relay exception and crash reports: models/relay_exception_schema.py and models/relay_exception.py, through the
shared exception pipeline (models/exception_fingerprint.py).

The validation rules are exercised as plain functions. The recording is exercised through the model's real entry point
(``_submit``) with the reporter's own cursor joined to the test, as the other exception tests do. The authenticated
relay-call route that feeds ``_submit`` is tested in ham_relay_bridge (hams_com).
"""
import copy
import html
import json
import uuid

import redis
from odoo.tests.common import tagged

from odoo.addons.hams_helpdesk.models import exception_fingerprint as reporter
from odoo.addons.hams_helpdesk.models import relay_exception as relay_model
from odoo.addons.hams_helpdesk.models import relay_exception_schema as schema

from .test_exception_members import MemberCase

FRAMES = [
    "std::panicking::begin_panic_handler::{{closure}} (/rustc/abc/library/std/src/panicking.rs:665)",
    "core::panicking::panic_fmt (/rustc/abc/library/core/src/panicking.rs:76)",
    "hams_local_relay::multiplexer::handle_command::{{closure}}::h0123456789abcdef (src/multiplexer.rs:123)",
    "tokio::runtime::task::core::Core<T,S>::poll (tokio-1.40/src/runtime/task/core.rs:310)",
]


def good(**overrides):
    """A valid report as the relay sends it."""
    report = dict(
        v=1,
        kind="panic",
        message="index out of bounds: the len is 3 but the index is 7",
        signal="",
        thread="tokio-runtime-worker",
        frames=list(FRAMES),
        version="0.3.4",
        build="1a2b3c4d",
        os="linux",
        arch="x86_64",
        uptime_s=3600,
        count=1,
        occurred_at=1_793_700_000,
        log_tail=["INFO started", "WARN something odd"],
        operator_key="",
    )
    report.update(overrides)
    return report


def rejected(test, call, *args, **kwargs):
    with test.assertRaises(schema.ReportRejected) as caught:
        call(*args, **kwargs)
    return caught.exception


@tagged("post_install", "-at_install", "standard")
class TestRelayReportSchema(MemberCase):
    # Tests [@ANCHOR: hams_helpdesk:relay_parse_body]
    # Tests [@ANCHOR: hams_helpdesk:relay_validate_report]
    # Tests [@ANCHOR: hams_helpdesk:relay_limits]
    # Tests [@ANCHOR: hams_helpdesk:relay_report_wire]
    # Tests [@ANCHOR: hams_helpdesk:relay_report_rejected]
    # Tests [@ANCHOR: hams_helpdesk:relay_report_rejected_init]
    # Tests [@ANCHOR: hams_helpdesk:relay_is_int]

    def test_a_valid_report_is_accepted_with_or_without_the_optional_key(self):
        self.assertEqual(schema.parse_body(json.dumps(good()).encode()), good())
        self.assertTrue(schema.validate_report(good(unsymbolised=True)))
        self.assertTrue(schema.validate_report(good(kind="signal", signal="SIGSEGV", operator_key="04" + "ab" * 64)))
        self.assertTrue(schema.validate_report(good(kind="error")))

    def test_each_bad_report_is_refused_with_its_own_reason(self):
        def without(key):
            report = good()
            del report[key]
            return report

        cases = [
            ("not an object", ["x"], "not_an_object"),
            ("a missing key", without("build"), "wrong_keys"),
            ("an extra key", good(extra=1), "wrong_keys"),
            ("a node id", good(node_id="abc"), "wrong_keys"),
            ("a wrong version", good(v=2), "wrong_version"),
            ("a boolean version", good(v=True), "wrong_version"),
            ("a wrong kind", good(kind="crash"), "wrong_kind"),
            ("a signal name on a panic", good(signal="SIGSEGV"), "wrong_signal"),
            ("SIGTERM is not a crash", good(kind="signal", signal="SIGTERM"), "wrong_signal"),
            ("SIGKILL is not a crash", good(kind="signal", signal="SIGKILL"), "wrong_signal"),
            ("a signal without a name", good(kind="signal"), "wrong_signal"),
            ("a number for a string", good(thread=5), "wrong_type"),
            ("a string for a number", good(count="1"), "wrong_type"),
            ("a boolean count", good(count=True), "wrong_type"),
            ("a non-boolean flag", good(unsymbolised="yes"), "wrong_type"),
            ("a long message", good(message="m" * 501), "too_long"),
            ("a long thread", good(thread="t" * 65), "too_long"),
            ("a version with spaces", good(version="1 2"), "wrong_version"),
            ("an empty build", good(build=""), "wrong_build"),
            ("an os with a slash", good(os="li/nux"), "wrong_os"),
            ("a long arch", good(arch="x" * 17), "wrong_arch"),
            ("an operator key that is not hex", good(operator_key="zz" * 32), "wrong_operator_key"),
            ("a short operator key", good(operator_key="ab" * 10), "wrong_operator_key"),
            ("a negative uptime", good(uptime_s=-1), "wrong_uptime"),
            ("a zero count", good(count=0), "wrong_count"),
            ("a huge count", good(count=schema.MAX_COUNT + 1), "wrong_count"),
            ("a zero time", good(occurred_at=0), "wrong_time"),
            ("no frames", good(frames=[]), "wrong_frames"),
            ("too many frames", good(frames=["a::b"] * (schema.MAX_FRAMES + 1)), "wrong_frames"),
            ("a long frame", good(frames=["a" * 241]), "wrong_frames"),
            ("a frame that is not text", good(frames=[1]), "wrong_frames"),
            ("a frame with a newline", good(frames=["a::b\nPassword: x"]), "wrong_frames"),
            ("log lines that are not a list", good(log_tail="x"), "wrong_log_tail"),
            ("too many log lines", good(log_tail=["x"] * (schema.MAX_LOG_LINES + 1)), "wrong_log_tail"),
            ("a long log line", good(log_tail=["x" * 241]), "wrong_log_tail"),
        ]
        for label, report, reason in cases:
            self.assertEqual(rejected(self, schema.validate_report, report).reason, reason, label)

    def test_the_body_is_size_capped_and_must_be_json_without_nul(self):
        too_large = json.dumps(good(log_tail=["x" * 240] * 20, frames=["a" * 240] * 25)).encode() + b" " * 16384
        self.assertEqual(rejected(self, schema.parse_body, too_large).status, 413)
        self.assertEqual(rejected(self, schema.parse_body, b"{broken").reason, "not_json")
        self.assertEqual(rejected(self, schema.parse_body, b'{"v": "a\\u0000"}').reason, "nul_byte")
        self.assertEqual(rejected(self, schema.parse_body, b'{"v": "a\x00"}').reason, "nul_byte")
        self.assertEqual(rejected(self, schema.parse_body, b"\xff\xfe").reason, "not_json")
        # An already-parsed dict is held to the same size.
        big = good(log_tail=["é" * 240] * 20, frames=["é" * 240] * 25, message="é" * 500)
        self.assertEqual(rejected(self, schema.validate_report, big).status, 413)


@tagged("post_install", "-at_install", "standard")
class TestRelayReportBuild(MemberCase):
    # Tests [@ANCHOR: hams_helpdesk:relay_normalize_frame]
    # Tests [@ANCHOR: hams_helpdesk:relay_top_frame]
    # Tests [@ANCHOR: hams_helpdesk:relay_build_report]
    # Tests [@ANCHOR: hams_helpdesk:relay_scrub_line]
    # Tests [@ANCHOR: hams_helpdesk:relay_report_tuple]
    # Tests [@ANCHOR: hams_helpdesk:relay_clean]
    # Tests [@ANCHOR: hams_helpdesk:relay_path_of]
    # Tests [@ANCHOR: hams_helpdesk:relay_function_label]

    def test_a_frame_is_normalised_to_its_function_path(self):
        cases = {
            "hams_local_relay::multiplexer::handle_command::h0123456789abcdef (src/multiplexer.rs:123)": "hams_local_relay::multiplexer::handle_command",
            "hams_local_relay::a::b::{{closure}} (src/a.rs:5:9)": "hams_local_relay::a::b::{closure}",
            "hams_local_relay::a::b::{closure#12}": "hams_local_relay::a::b::{closure}",
            "alloc::vec::Vec<T,A>::push": "alloc::vec::Vec<>::push",
            "core::ptr::drop_in_place<tokio::task::Core<hams_local_relay::x::Fut<u8>>>": "core::ptr::drop_in_place<>",
            "<unresolved> +0x1f2e3d": "<unresolved>",
            "hams_local_relay::a::b +0x4af": "hams_local_relay::a::b",
            "0x7ffe12345678": "<unresolved>",
        }
        for raw, expected in cases.items():
            self.assertEqual(schema.normalize_frame(raw), expected, raw)

    def test_the_blamed_frame_is_the_first_of_the_relay_not_the_runtime(self):
        frame, module = schema.top_frame(FRAMES)
        self.assertEqual(frame, "hams_local_relay::multiplexer::handle_command::{closure}")
        self.assertEqual(module, "multiplexer")
        # Another crate's frame when none is the relay's; the first when all are the runtime's.
        frame, module = schema.top_frame(["std::panicking::x", "other_crate::thing::run (a.rs:1)"])
        self.assertEqual((frame, module), ("other_crate::thing::run", "other_crate"))
        frame, module = schema.top_frame(["<unresolved> +0x10", "<unresolved> +0x20"])
        self.assertEqual((frame, module), ("<unresolved>", "<unresolved>"))
        # The crash reporter's own frames are not blamed.
        frame, _module = schema.top_frame(
            ["hams_local_relay::crash_report::handler (src/crash_report.rs:9)", "hams_local_relay::radio::poll (src/radio.rs:2)"]
        )
        self.assertEqual(frame, "hams_local_relay::radio::poll")

    def test_the_fingerprint_inputs_are_the_same_across_builds_addresses_lines_and_messages(self):
        first = schema.build_report(good(), "relay-one")
        other_frames = [
            "std::panicking::begin_panic_handler::{{closure}} (/rustc/zzz/library/std/src/panicking.rs:700)",
            "core::panicking::panic_fmt (/rustc/zzz/library/core/src/panicking.rs:99)",
            "hams_local_relay::multiplexer::handle_command::{{closure#4}}::hfedcba9876543210 (src/multiplexer.rs:456)",
        ]
        second = schema.build_report(
            good(frames=other_frames, build="ffff", version="0.4.0", message="a different text", count=9, thread="main", uptime_s=1),
            "relay-two",
        )
        self.assertEqual(first.key_parts, second.key_parts)
        self.assertEqual(first.key_parts, ("relay.panic", "multiplexer", "hams_local_relay::multiplexer::handle_command::{closure}", "relay"))
        # Different function, kind or signal: different failures.
        moved = schema.build_report(good(frames=["hams_local_relay::audio::open (src/audio.rs:1)"]), "r")
        self.assertNotEqual(first.key_parts, moved.key_parts)
        sig = schema.build_report(good(kind="signal", signal="SIGSEGV"), "r")
        sig2 = schema.build_report(good(kind="signal", signal="SIGBUS"), "r")
        self.assertEqual(sig.key_parts[0], "relay.signal.SIGSEGV")
        self.assertNotEqual(sig.key_parts, sig2.key_parts)
        self.assertNotEqual(first.key_parts[0], sig.key_parts[0])

    def test_the_names_say_what_happened_and_where(self):
        self.assertEqual(schema.build_report(good(), "r").name, "panic in multiplexer::handle_command::{closure}")
        self.assertEqual(schema.build_report(good(kind="error"), "r").name, "error in multiplexer::handle_command::{closure}")
        self.assertEqual(schema.build_report(good(kind="signal", signal="SIGSEGV"), "r").name, "SIGSEGV in multiplexer::handle_command::{closure}")

    def test_the_trace_carries_the_facts_and_scrubs_planted_secrets(self):
        secrets = [
            "hunter2", "sk_live_abcdefghijklmnop", "Zm9vYmFyYmF6cXV4", "k6bp@example.com", "198.51.100.7",
            "s3cretvalue", "eyJhbGciOiJIUzI1NiJ9abcdefghijklmnopqrstuv",
        ]
        report = schema.build_report(
            good(
                message="login failed password=hunter2 for k6bp@example.com from 198.51.100.7 api_key=sk_live_abcdefghijklmnop",
                log_tail=[
                    "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9abcdefghijklmnopqrstuv",
                    "connect telnet://K6BP:s3cretvalue@cms.example:8772 token=Zm9vYmFyYmF6cXV4",
                    "ok",
                ],
                frames=["hams_local_relay::multiplexer::handle_command (src/a.rs:1)", "x::y?password=s3cretvalue"],
            ),
            "relay-abc",
        )
        for secret in secrets:
            self.assertNotIn(secret, report.trace, secret)
        for fact in ("Relay id: relay-abc", "0.3.4", "build 1a2b3c4d", "linux x86_64", "tokio-runtime-worker", "Uptime: 3600", "occurrences since its last report: 1", "UTC", "hams_local_relay::multiplexer::handle_command"):
            self.assertIn(fact, report.trace, fact)
        self.assertIn("<redacted>", report.trace)

    def test_scrub_line_removes_control_characters_and_cuts(self):
        text = schema.scrub_line("a\x1b[31mred\x07\nnext password=zzz", 20)
        self.assertNotIn("\x1b", text)
        self.assertNotIn("\n", text)
        self.assertLessEqual(len(text), 20)
        self.assertNotIn("zzz", schema.scrub_line("a\x1b[31mred\x07\nnext password=zzz", 200))


@tagged("post_install", "-at_install", "standard")
class TestRelayExceptionRecording(MemberCase):
    # Tests [@ANCHOR: hams_helpdesk:relay_submit]
    # Tests [@ANCHOR: hams_helpdesk:relay_record]
    # Tests [@ANCHOR: hams_helpdesk:exc_report_event]
    # Tests [@ANCHOR: hams_helpdesk:exc_sync_ticket]
    # Tests [@ANCHOR: hams_helpdesk:exc_ticket_body]

    def setUp(self):
        super().setUp()
        self.Relay = self.env["hams_helpdesk.relay.exception"]
        # A fresh relay id per test: the rate-limit counters live in Redis across tests.
        self.relay_id = "relay-" + uuid.uuid4().hex

    def submit(self, report=None, owner=None, operator=None, relay_id=None):
        self.env.flush_all()
        result = self.Relay._submit(
            report if report is not None else good(), relay_id or self.relay_id, owner.id if owner else None, operator.id if operator else None
        )
        self.env.invalidate_all()
        return result

    def rows(self):
        return self.Fingerprint.search([("source", "=", "relay")])

    def test_a_panic_makes_one_relay_exception_ticket_with_the_stack(self):
        self.assertEqual(self.submit(), relay_model.RECORDED)
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.exc_type, "relay.panic")
        self.assertEqual(rows.module, "multiplexer")
        self.assertEqual(rows.route, "relay")
        ticket = rows.ticket_id
        self.assertEqual(ticket.ticket_type, "server_exception")
        self.assertEqual(ticket.name, schema.TITLE_PREFIX_EXCEPTION + "panic in multiplexer::handle_command::{closure}")
        self.assertTrue(ticket.name.startswith("Relay exception: "))
        description = html.unescape(ticket.description)
        self.assertIn("hams_local_relay", description)
        self.assertIn(self.relay_id, description)
        self.assertIn("No audio and no radio traffic are collected", description)
        self.assertNotIn("privacy", description.lower())

    def test_a_fatal_signal_makes_a_relay_crash_ticket(self):
        self.assertEqual(self.submit(good(kind="signal", signal="SIGSEGV", message="fatal signal")), relay_model.RECORDED)
        ticket = self.rows().ticket_id
        self.assertEqual(ticket.name, schema.TITLE_PREFIX_CRASH + "SIGSEGV in multiplexer::handle_command::{closure}")
        self.assertTrue(ticket.name.startswith("Relay crash: "))

    def test_the_same_crash_from_two_builds_and_relays_is_one_ticket_with_a_counter(self):
        self.assertEqual(self.submit(good(count=3)), relay_model.RECORDED)
        rows = self.rows()
        self.assertEqual(rows.occurrence_count, 3, "the relay's own count of repeats is carried")
        other_frames = [
            "hams_local_relay::multiplexer::handle_command::{{closure#2}}::h1111111111111111 (src/multiplexer.rs:999)"
        ]
        self.assertEqual(
            self.submit(good(frames=other_frames, build="beef", version="0.4.0"), relay_id="relay-" + uuid.uuid4().hex),
            relay_model.RECORDED,
        )
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(len(self.env["hams_helpdesk.ticket"].search([("name", "=like", "Relay exception:%")])), 1)
        # The second came inside the once-a-minute window: held back and counted for the next write.
        self.assertEqual(rows.occurrence_count, 3)
        self.assertEqual(reporter._GATE[rows.fingerprint][1], 1)

    def test_the_owner_and_the_operator_are_linked_and_shown_with_name_callsign_and_email(self):
        self.assertEqual(self.submit(owner=self.member, operator=self.portal_other), relay_model.RECORDED)
        rows = self.rows()
        self.assertEqual(rows.member_ids.user_id, self.member + self.portal_other)
        panel = self.the_panel(rows)
        owner_row = panel.filtered(lambda row: row.user_id == self.member)
        self.assertEqual(owner_row.member_name, "Marguerite Quillfeather")
        self.assertEqual(owner_row.contact_email, "marguerite.quillfeather@example.net")
        self.assertIn(self.callsign.upper(), owner_row.callsigns)
        # Linked, never copied into the text.
        stored = " ".join([rows.name, rows.samples_json, rows.ticket_id.description, rows.ticket_id.name])
        for leaked in ("Quillfeather", "example.net", "exc_member_marguerite"):
            self.assertNotIn(leaked, stored)

    def test_the_operator_alone_is_linked_when_the_relay_has_no_owner_account(self):
        self.assertEqual(self.submit(operator=self.member), relay_model.RECORDED)
        self.assertEqual(self.rows().member_ids.user_id, self.member)

    def test_an_unlinked_relay_is_anonymous_and_its_id_is_in_the_trace(self):
        self.assertEqual(self.submit(), relay_model.RECORDED)
        rows = self.rows()
        self.assertFalse(rows.member_ids)
        self.assertEqual(rows.last_user_id, 0)
        self.assertIn(self.relay_id, rows.samples_json)

    def test_a_service_or_public_account_is_never_linked_as_a_member(self):
        service = self.env["zero_sudo.security.utils"]._get_service_env(reporter.SERVICE_XMLID).user
        self.assertEqual(self.submit(owner=self.env.ref("base.public_user"), operator=service), relay_model.RECORDED)
        self.assertFalse(self.rows().member_ids)

    def test_a_second_member_is_listed_at_once_inside_the_window(self):
        self.submit(owner=self.member)
        self.assertEqual(self.submit(operator=self.portal_other), relay_model.RECORDED)
        self.assertEqual(self.rows().member_ids.user_id, self.member + self.portal_other)

    def test_secrets_planted_in_every_text_field_never_reach_the_database(self):
        planted = "password=hunter2 api_key=AKIAABCDEFGHIJKLMNOP Authorization: Bearer abc.def.ghi-jklmnopqrstuvwx k6bp@example.com"
        report = good(message=planted, thread="t-k6bp@example.com", log_tail=[planted, "telnet://K6BP:hunter2@cms.example:8772"])
        self.assertEqual(self.submit(report), relay_model.RECORDED)
        rows = self.rows()
        stored = " ".join([rows.name, rows.frame, rows.samples_json, rows.ticket_id.description, rows.ticket_id.name])
        for secret in ("hunter2", "AKIAABCDEFGHIJKLMNOP", "abc.def.ghi", "k6bp@example.com", "K6BP:"):
            self.assertNotIn(secret, stored, secret)

    def test_a_bad_report_is_refused_and_records_nothing(self):
        for bad in (good(v=2), good(extra=1), {"x": 1}, "text"):
            self.assertEqual(self.submit(bad), relay_model.REJECTED)
        self.assertFalse(self.rows())

    def test_a_relay_over_the_rate_limit_is_limited_and_another_is_not(self):
        # Tests [@ANCHOR: hams_helpdesk:relay_rate_limited]
        # Tests [@ANCHOR: hams_helpdesk:relay_client_key]
        allowed = relay_model.RATE_LIMITS[0][1]
        results = [self.submit(good(v=2)) for _ in range(allowed + 2)]
        self.assertEqual(results[:allowed], [relay_model.REJECTED] * allowed)
        self.assertEqual(results[allowed:], [relay_model.LIMITED] * 2)
        self.assertEqual(self.submit(good(v=2), relay_id="relay-" + uuid.uuid4().hex), relay_model.REJECTED)
        self.assertNotEqual(self.Relay._client_key("a"), self.Relay._client_key("b"))
        self.assertNotIn(self.relay_id, self.Relay._client_key(self.relay_id))

    def test_when_the_limiter_cannot_count_the_report_is_dropped(self):
        # Tests [@ANCHOR: hams_helpdesk:relay_rate_limited]
        self.safe_patch_object(relay_model, "get_redis_connection", side_effect=redis.ConnectionError("down"))
        self.assertEqual(self.submit(), relay_model.LIMITED)
        self.assertFalse(self.rows())

    def test_the_relay_tickets_have_their_own_hourly_cap(self):
        cap = reporter.RELAY_NEW_TICKETS_PER_HOUR
        for index in range(cap + 2):
            frames = [f"hams_local_relay::mod{index}::run (src/m.rs:1)"]
            self.assertEqual(self.submit(good(frames=frames), relay_id="relay-" + uuid.uuid4().hex), relay_model.RECORDED)
        rows = self.rows()
        self.assertEqual(len(rows), cap + 2, "every failure is counted")
        self.assertEqual(len(rows.filtered("ticket_id")), cap, "but only the cap opens tickets")

    def test_a_failure_inside_the_recorder_is_not_reported_by_the_recorder(self):
        self.safe_patch_object(type(self.Relay), "_record", side_effect=RuntimeError("x"))
        with self.assertLogs("hams_helpdesk.exception_reporter", level="ERROR"):
            self.assertEqual(self.submit(), relay_model.REJECTED)
        self.assertFalse(self.rows())

    def test_existing_callers_of_report_event_are_unchanged_by_the_extra_arguments(self):
        # Tests [@ANCHOR: hams_helpdesk:exc_admit]
        self.assertEqual(reporter._admit("fp-x"), 0)
        self.assertEqual(reporter._admit("fp-y", "check", 5), 0)
        self.assertIsNone(reporter._admit("fp-y", "check", 5), "the same member again inside the window is held back")
        self.assertEqual(reporter._admit("fp-y", "check", (5, 6)), 1, "a new member in the tuple is admitted at once")
        self.assertEqual(copy.deepcopy(reporter.MAX_EXTRA_UIDS), 4)
