# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Browser JavaScript error reports: controllers/browser_exception_api.py, models/browser_exception.py and
models/browser_exception_schema.py, through the shared exception pipeline (models/exception_fingerprint.py).

The validation rules are exercised as plain functions; the endpoint is exercised over real HTTP with real
commits (the report is written on its own cursor), including the checks that stand in for a nonce: same-origin
signals and a current asset bundle version.
"""
import json
import re
import uuid

import redis
from odoo.modules.module import get_manifest
from odoo.tests.common import tagged

from odoo.addons.hams_helpdesk.controllers import browser_exception_api as api
from odoo.addons.hams_helpdesk.models import browser_exception as browser_model
from odoo.addons.hams_helpdesk.models import browser_exception_schema as schema
from odoo.addons.hams_helpdesk.models import exception_fingerprint as reporter
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

BUNDLE = "web.assets_frontend"
OWN_HOST = "hams.example"


def good_body(**overrides):
    """A valid report whose top frame is a script of OWN_HOST."""
    body = dict(
        v=1,
        type="TypeError",
        message="Cannot read properties of null (reading 'x')",
        frames=[f"https://{OWN_HOST}/web/assets/1a2b3c4/{BUNDLE}.min.js:10:20"],
        route="/community/<x>",
        agent="Firefox",
        release=f"{BUNDLE}@1a2b3c4",
    )
    body.update(overrides)
    return body


def encode(body):
    return json.dumps(body).encode()


def rejected(test, call, *args, **kwargs):
    """The ReportRejected ``call`` raises, for the test to inspect."""
    with test.assertRaises(schema.ReportRejected) as caught:
        call(*args, **kwargs)
    return caught.exception


@tagged("post_install", "-at_install", "standard")
class TestBrowserReportSchema(HamsTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:browser_parse_body]
    # Tests [@ANCHOR: hams_helpdesk:browser_parse_frame]
    # Tests [@ANCHOR: hams_helpdesk:browser_frame_key]
    # Tests [@ANCHOR: hams_helpdesk:browser_agent_family]
    # Tests [@ANCHOR: hams_helpdesk:browser_is_bot_agent]
    # Tests [@ANCHOR: hams_helpdesk:browser_is_ignorable]
    # Tests [@ANCHOR: hams_helpdesk:browser_stored_message]
    # Tests [@ANCHOR: hams_helpdesk:browser_build_report]

    def test_a_valid_body_is_accepted_and_every_other_shape_is_refused(self):
        self.assertEqual(schema.parse_body(encode(good_body()))["type"], "TypeError")
        cases = {
            "too_large": encode(good_body(message="m" * 9000)),
            "nul_byte": encode(good_body(message="a\x00b")),
            "not_json": b"{not json",
            "not_an_object": b"[1, 2]",
            "wrong_keys": encode(dict(good_body(), extra="x")),
            "wrong_version": encode(good_body(v=2)),
            "wrong_type": encode(good_body(type=5)),
            "wrong_frames": encode(good_body(frames=[])),
            "too_long": encode(good_body(message="m" * 1001)),
        }
        for reason, raw in cases.items():
            self.assertEqual(rejected(self, schema.parse_body, raw).reason, reason, reason)
        too_many = good_body(frames=[f"/web/assets/1a2b3c4/{BUNDLE}.min.js:1:1"] * 9)
        self.assertEqual(rejected(self, schema.parse_body, encode(too_many)).reason, "wrong_frames")
        self.assertEqual(rejected(self, schema.parse_body, encode(good_body(frames=[7]))).reason, "wrong_frames")
        self.assertEqual(rejected(self, schema.parse_body, b"\xff\xfe").reason, "not_json")
        missing = good_body()
        del missing["route"]
        self.assertEqual(rejected(self, schema.parse_body, encode(missing)).reason, "wrong_keys")

    def test_only_this_sites_own_scripts_are_accepted(self):
        asset = f"https://{OWN_HOST}/web/assets/1a2b3c4/{BUNDLE}.min.js:10:20"
        frame = schema.parse_frame(asset, OWN_HOST)
        self.assertEqual((frame.line, frame.col, frame.bundle, frame.version, frame.module), (10, 20, BUNDLE, "1a2b3c4", BUNDLE))
        static = schema.parse_frame("/ham_base/static/src/js/a.js:3:4", OWN_HOST)
        self.assertEqual((static.module, static.version), ("ham_base", None))
        self.assertEqual(schema.parse_frame("/web/assets/1a2b3c4/web.assets_backend.min.js:1:1").bundle, "web.assets_backend")
        foreign = [
            "chrome-extension://abcdefgh/content.js:1:2",
            "moz-extension://abcd/content.js:1:2",
            f"https://evil.example/web/assets/1a2b3c4/{BUNDLE}.min.js:1:2",
            f"https://user:pw@{OWN_HOST}/web/assets/1a2b3c4/{BUNDLE}.min.js:1:2",
            "blob:https://hams.example/uuid:1:2",
            "data:text/javascript,alert(1):1:2",
            f"https://{OWN_HOST}/some/other/page.js:1:2",
            f"https://{OWN_HOST}/web/assets/1a2b3c4/{BUNDLE}.min.css:1:2",
        ]
        for text in foreign:
            self.assertEqual(rejected(self, schema.parse_frame, text, OWN_HOST).reason, "foreign_script", text)
        self.assertEqual(rejected(self, schema.parse_frame, "https://hams.example/a.js:1:2", None).reason, "foreign_script")
        for text in (
            f"/web/assets/1a2b3c4/{BUNDLE}.min.js?token=zz:1:2",
            f"/web/assets/1a2b3c4/{BUNDLE}.min.js#frag:1:2",
            "/ham_base/static/../../etc/passwd.js:1:2",
            "no line numbers",
        ):
            self.assertEqual(rejected(self, schema.parse_frame, text, OWN_HOST).status, 400, text)

    def test_the_fingerprint_frame_drops_the_bundle_version_and_keeps_line_and_column(self):
        old = schema.parse_frame(f"/web/assets/1a2b3c4/{BUNDLE}.min.js:10:20")
        new = schema.parse_frame(f"/web/assets/9f8e7d6/{BUNDLE}.min.js:10:20")
        self.assertEqual(schema.frame_key(old), schema.frame_key(new))
        self.assertEqual(schema.frame_key(old), f"/web/assets/*/{BUNDLE}.min.js:10:20")
        self.assertNotEqual(schema.frame_key(old), schema.frame_key(schema.parse_frame(f"/web/assets/1a2b3c4/{BUNDLE}.min.js:10:21")))
        self.assertEqual(schema.frame_key(schema.parse_frame("/ham_base/static/a.js:1:2")), "/ham_base/static/a.js:1:2")

    def test_agent_is_a_family_from_the_fixed_list(self):
        self.assertEqual(schema.agent_family("Chrome"), "Chrome")
        for value in ("Mozilla/5.0 (X11; Linux x86_64) Firefox/130.0", "chrome", ""):
            self.assertEqual(rejected(self, schema.agent_family, value).reason, "wrong_agent")

    def test_bots_and_command_line_clients_are_recognised(self):
        for agent in ("Googlebot/2.1", "curl/8.0", "python-requests/2.31", "Mozilla/5.0 (compatible; Scanner)"):
            self.assertTrue(schema.is_bot_agent(agent), agent)
        self.assertFalse(schema.is_bot_agent("Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0"))
        self.assertFalse(schema.is_bot_agent(""))

    def test_noise_is_ignored(self):
        self.assertTrue(schema.is_ignorable("RPC_ERROR", "x"))
        self.assertTrue(schema.is_ignorable("Error", "ResizeObserver loop completed with undelivered notifications."))
        self.assertTrue(schema.is_ignorable("TypeError", "Failed to fetch"))
        self.assertFalse(schema.is_ignorable("TypeError", "Cannot read properties of null"))
        self.assertEqual(rejected(self, schema.build_report, good_body(type="RPC_ERROR"), OWN_HOST).status, 204)

    def test_messages_are_kept_only_for_builtin_classes_and_scrubbed(self):
        secret = "k6bp@example.com password=hunter2 15551234567 see https://hams.com/x?session_id=zzz K6BP"
        text = schema.stored_message("TypeError", f"bad {secret}\nsecond line")
        for leaked in ("example.com", "hunter2", "15551234567", "session_id", "K6BP"):
            self.assertNotIn(leaked, text)
        self.assertLessEqual(len(schema.stored_message("TypeError", "m" * 900)), schema.STORED_MESSAGE_CHARS)
        self.assertEqual(schema.stored_message("Error", secret), "<message withheld>")
        self.assertEqual(schema.stored_message("UnhandledRejection", secret), "<message withheld>")

    def test_build_report_assembles_only_validated_pieces(self):
        body = good_body(
            frames=[
                f"https://{OWN_HOST}/web/assets/1a2b3c4/{BUNDLE}.min.js:10:20",
                "chrome-extension://abcd/wrapper.js:1:2",
                f"https://{OWN_HOST}/web/assets/1a2b3c4/{BUNDLE}.min.js:30:40",
            ],
            route="/community/k6bp/12345",
        )
        report = schema.build_report(body, OWN_HOST)
        self.assertEqual(report.type_name, "browser.TypeError")
        self.assertEqual(report.module, BUNDLE)
        self.assertEqual(report.frame, f"/web/assets/*/{BUNDLE}.min.js:10:20")
        self.assertEqual(report.route, "/community/<x>/<x>")
        self.assertEqual((report.bundle, report.version, report.agent), (BUNDLE, "1a2b3c4", "Firefox"))
        self.assertIn("Stack (innermost first", report.trace)
        self.assertIn(f"  /web/assets/1a2b3c4/{BUNDLE}.min.js:30:40", report.trace)
        self.assertNotIn("extension", report.trace)
        self.assertNotIn("hams.example", report.trace)

    def test_build_report_refuses_what_it_cannot_vouch_for(self):
        def build(**changes):
            return schema.build_report(good_body(**changes), OWN_HOST)

        self.assertEqual(rejected(self, build, type="bad type!").reason, "wrong_type_name")
        self.assertEqual(rejected(self, build, release="nonsense").reason, "wrong_release")
        self.assertEqual(rejected(self, build, agent="Mozilla").reason, "wrong_agent")
        self.assertEqual(rejected(self, build, route="/a?x=1").reason, "wrong_route")
        self.assertEqual(rejected(self, build, route="nope").reason, "wrong_route")
        self.assertEqual(rejected(self, build, release=f"{BUNDLE}@9f8e7d6").reason, "release_mismatch")
        self.assertEqual(
            rejected(self, build, frames=["chrome-extension://abcd/wrapper.js:1:2"]).reason, "foreign_script"
        )
        self.assertEqual(rejected(self, schema.build_report, good_body(), OWN_HOST, True).reason, "bot")


@tagged("post_install", "-at_install", "standard")
class TestBrowserExceptionEndpoint(RealTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:browser_exception_endpoint]
    # Tests [@ANCHOR: hams_helpdesk:browser_same_origin]
    # Tests [@ANCHOR: hams_helpdesk:browser_record]
    # Tests [@ANCHOR: hams_helpdesk:browser_release_is_current]
    # Tests [@ANCHOR: hams_helpdesk:browser_current_versions]

    def setUp(self):
        super().setUp()
        for state in (reporter._GATE, reporter._RECENT_WRITES, browser_model._VERSIONS):
            state.clear()
            self.addCleanup(state.clear)
        self.Fingerprint = self.env["hams_helpdesk.exception.fingerprint"]
        self.version = self.env["hams_helpdesk.browser.exception"]._current_versions()[BUNDLE]
        self.origin = self.base_url()
        self.host = self.origin.split("://", 1)[1]
        # A fresh visitor address per test: the rate-limit counters live in Redis across tests.
        self.visitor = f"198.51.100.{uuid.uuid4().int % 238 + 13}"
        self.user_agent = "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0"

    def body(self, line=10, **overrides):
        values = {
            "frames": [f"{self.origin}/web/assets/{self.version}/{BUNDLE}.min.js:{line}:20"],
            "release": f"{BUNDLE}@{self.version}",
        }
        values.update(overrides)
        return good_body(**values)

    def post(self, body=None, raw=None, headers=None, visitor=None):
        sent = {
            "Content-Type": "application/json",
            "CF-Connecting-IP": visitor or self.visitor,
            "User-Agent": self.user_agent,
        }
        sent.update(headers or {})
        if raw is None:
            raw = encode(body if body is not None else self.body())
        return self.url_open(api.ROUTE, data=raw, headers=sent, allow_redirects=False)

    def rows(self, domain=()):
        self.env.cr.rollback()  # a fresh snapshot: the reporter committed on its own connection
        self.env.invalidate_all()
        return self.Fingerprint.search([("source", "=", "browser")] + list(domain))

    def test_a_report_makes_one_ticket_that_says_browser_and_shows_the_stack(self):
        # Tests [@ANCHOR: hams_helpdesk:exc_ticket_body]
        response = self.post()
        self.assertEqual(response.status_code, 204)
        self.assertEqual(response.content, b"")
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.source, "browser")
        self.assertEqual(rows.exc_type, "browser.TypeError")
        self.assertEqual(rows.module, BUNDLE)
        self.assertEqual(rows.route, "/community/<x>")
        self.assertEqual(rows.ticket_id.ticket_type, "server_exception")
        self.assertTrue(rows.ticket_id.name.startswith("Browser exception: "))
        description = rows.ticket_id.description
        self.assertIn("Browser", description)
        self.assertIn("JavaScript error reported by a browser", description)
        self.assertIn(f"/web/assets/{self.version}/{BUNDLE}.min.js:10:20", description)
        self.assertNotIn("<li><b>Source:</b> browser</li>", description)

    def test_repeats_dedupe_to_one_ticket_and_the_count_is_carried_into_the_next_write(self):
        for _ in range(3):
            self.assertEqual(self.post().status_code, 204)
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.occurrence_count, 1, "the once-a-minute gate holds the repeats back")
        # A minute later: one write carries the two held-back hits and this one.
        uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_exception_reporter_service")
        service_rows = rows.with_user(uid)
        service_rows.last_written_at = service_rows.last_written_at - reporter.timedelta(seconds=120)
        self.env.cr.commit()
        reporter._GATE[rows.fingerprint][0] -= 120
        self.assertEqual(self.post().status_code, 204)
        rows = self.rows()
        self.assertEqual(rows.occurrence_count, 4)
        self.assertEqual(len(self.env["hams_helpdesk.ticket"].search([("name", "=like", "Browser exception:%")])), 1)
        # A different line is a different failure.
        self.post(self.body(line=11))
        self.assertEqual(len(self.rows()), 2)

    def test_nothing_personal_reaches_the_database(self):
        self.authenticate("admin", "admin")
        secret = "k6bp@example.com password=hunter2 15551234567 K6BP https://hams.com/x?session_id=zzz&email=a@b.com"
        body = self.body(message=f"Cannot read {secret}", route="/member/k6bp/1234567")
        self.assertEqual(self.post(body, headers={"Cookie": "session_id=deadbeef" * 3}).status_code, 204)
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.last_user_id, 0, "a session id the server does not know is nobody")
        self.assertFalse(rows.member_ids)
        stored = " ".join(
            [rows.name, rows.frame, rows.route, rows.samples_json, rows.reporters_json, rows.ticket_id.description]
            + [str(rows.ticket_id.name)]
        )
        for leaked in ("example.com", "hunter2", "15551234567", "session_id", "K6BP", "k6bp", "deadbeef", self.visitor):
            self.assertNotIn(leaked, stored, leaked)
        self.assertNotRegex(rows.frame + rows.route, r"\?")
        self.assertEqual(json.loads(rows.samples_json)[0]["uid"], None)

    # Tests [@ANCHOR: hams_helpdesk:exc_note_member]
    def test_a_signed_in_member_is_linked_by_the_session_cookie_and_nothing_else_is_sent(self):
        admin = self.env.ref("base.user_admin")
        self.authenticate("admin", "admin")
        body = self.body()
        self.assertEqual(sorted(body), sorted(good_body()), "the page sends no identity of its own")
        self.assertEqual(self.post(body).status_code, 204)
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows.member_ids.user_id, admin)
        self.assertEqual(rows.last_member_id, admin)
        self.assertEqual(rows.last_user_id, admin.id)
        stored = " ".join([rows.name, rows.frame, rows.route, rows.samples_json, str(rows.ticket_id.description)])
        for leaked in (admin.name, admin.login, "admin@", self.visitor):
            self.assertNotIn(leaked, stored, leaked)
        self.assertIn("the member's account is linked to this report", rows.ticket_id.description)

    def test_a_logged_out_visitor_stays_anonymous(self):
        self.assertEqual(self.post().status_code, 204)
        rows = self.rows()
        self.assertEqual(len(rows), 1)
        self.assertFalse(rows.member_ids)
        self.assertFalse(rows.last_member_id)
        self.assertEqual(rows.last_user_id, 0)
        self.assertEqual(json.loads(rows.samples_json)[0]["uid"], None)

    def test_a_cross_site_post_with_the_members_cookie_is_refused_and_links_nobody(self):
        self.authenticate("admin", "admin")
        for header in ({"Sec-Fetch-Site": "cross-site"}, {"Origin": "https://evil.example"}):
            self.assertEqual(self.post(headers=header).status_code, 403)
        self.assertFalse(self.rows())

    def test_a_custom_error_class_keeps_its_stack_but_not_its_message(self):
        self.post(self.body(type="ValidationFailure", message="callsign K6BP rejected for pat@example.com"))
        description = self.rows().ticket_id.description
        self.assertIn("&lt;message withheld&gt;", description)
        self.assertNotIn("K6BP", description)
        self.assertNotIn("pat@example.com", description)

    def test_bad_requests_are_refused_and_record_nothing(self):
        too_large = encode(self.body(message="m" * 900)) + b" " * 9000
        cases = [
            ("not json", dict(raw=b"{broken"), 400),
            ("wrong keys", dict(body=dict(self.body(), extra=1)), 400),
            ("too large", dict(raw=too_large), 413),
            ("wrong content type", dict(headers={"Content-Type": "text/plain"}), 415),
            ("foreign script", dict(body=self.body(frames=["chrome-extension://abcd/content.js:1:2"])), 422),
            ("another site", dict(body=self.body(frames=[f"https://evil.example/web/assets/{self.version}/{BUNDLE}.min.js:1:2"])), 422),
            ("a query string", dict(body=self.body(frames=[f"{self.origin}/web/assets/{self.version}/{BUNDLE}.min.js?x=1:1:2"])), 400),
            ("a bot", dict(headers={"User-Agent": "Googlebot/2.1"}), 422),
        ]
        for label, kwargs, status in cases:
            self.assertEqual(self.post(visitor=f"198.51.100.{200 + len(label) % 50}", **kwargs).status_code, status, label)
        self.assertIn(self.url_open(api.ROUTE, headers={"CF-Connecting-IP": self.visitor}).status_code, (404, 405))
        self.assertFalse(self.rows())

    def test_noise_is_accepted_quietly_and_not_recorded(self):
        for body in (self.body(type="RPC_ERROR"), self.body(message="ResizeObserver loop limit exceeded")):
            self.assertEqual(self.post(body).status_code, 204)
        self.assertFalse(self.rows())

    def test_a_stale_or_unknown_bundle_version_is_refused(self):
        # Tests [@ANCHOR: hams_helpdesk:browser_release_is_current]
        stale = "0000000" if self.version != "0000000" else "1111111"
        stale_body = self.body(
            frames=[f"{self.origin}/web/assets/{stale}/{BUNDLE}.min.js:1:2"], release=f"{BUNDLE}@{stale}"
        )
        self.assertEqual(self.post(stale_body).status_code, 422)
        # Right version, but the release names a bundle that is not one of the page bundles.
        unknown = self.body(
            frames=[f"{self.origin}/web/assets/{self.version}/my.other_bundle.min.js:1:2"],
            release=f"my.other_bundle@{self.version}",
        )
        self.assertEqual(self.post(unknown).status_code, 422)
        # The frame and the release must agree.
        other = "0000000" if self.version != "0000000" else "1111111"
        self.assertEqual(self.post(self.body(release=f"{BUNDLE}@{other}")).status_code, 422)
        self.assertFalse(self.rows())
        self.assertEqual(self.post(self.body()).status_code, 204)
        self.assertEqual(len(self.rows()), 1)

    def test_a_cross_site_request_is_refused_and_a_same_origin_one_accepted(self):
        for headers in (
            {"Sec-Fetch-Site": "cross-site"},
            {"Sec-Fetch-Site": "same-site"},
            {"Sec-Fetch-Site": "none"},
            {"Origin": "https://evil.example"},
            {"Origin": "null"},
            {"Sec-Fetch-Site": "same-origin", "Origin": "https://evil.example"},
        ):
            self.assertEqual(self.post(headers=headers).status_code, 403, headers)
        self.assertFalse(self.rows())
        self.assertEqual(self.post(headers={"Sec-Fetch-Site": "same-origin", "Origin": self.origin}).status_code, 204)
        self.assertEqual(len(self.rows()), 1)
        # No browser headers at all (curl, an old browser): accepted, left to the other defences.
        self.assertEqual(self.post(self.body(line=99)).status_code, 204)

    def test_a_visitor_over_the_rate_limit_is_refused_and_another_is_not(self):
        # Tests [@ANCHOR: hams_helpdesk:browser_rate_limited]
        # Tests [@ANCHOR: hams_helpdesk:browser_client_key]
        allowed = browser_model.RATE_LIMITS[0][1]
        statuses = [self.post(raw=b"{broken").status_code for _ in range(allowed + 2)]
        self.assertEqual(statuses[:allowed], [400] * allowed)
        self.assertEqual(statuses[allowed:], [429, 429])
        other = f"198.51.100.{(int(self.visitor.rsplit('.', 1)[1]) % 250) + 1}"
        self.assertEqual(self.post(raw=b"{broken", visitor=other).status_code, 400)

    def test_when_the_limiter_cannot_count_the_report_is_dropped(self):
        # Tests [@ANCHOR: hams_helpdesk:browser_rate_limited]
        self.safe_patch_object(browser_model, "get_redis_connection", side_effect=redis.ConnectionError("down"))
        self.assertEqual(self.post().status_code, 429)
        self.assertFalse(self.rows())

    def test_the_browser_tickets_have_their_own_hourly_cap(self):
        # Tests [@ANCHOR: hams_helpdesk:exc_sync_ticket]
        cap = reporter.BROWSER_NEW_TICKETS_PER_HOUR
        for line in range(1, cap + 3):
            self.assertEqual(self.post(self.body(line=line), visitor=f"198.51.100.{line}").status_code, 204)
        rows = self.rows()
        self.assertEqual(len(rows), cap + 2, "every failure is counted")
        self.assertEqual(len(rows.filtered("ticket_id")), cap, "but only the cap opens tickets")

    def test_a_failure_inside_the_beacon_is_not_reported_by_the_beacon(self):
        self.safe_patch_object(type(self.env["hams_helpdesk.browser.exception"]), "_record", side_effect=RuntimeError("x"))
        with self.assertLogs("hams_helpdesk.exception_reporter", level="ERROR"):
            self.assertEqual(self.post().status_code, 503)
        self.assertFalse(self.rows())


@tagged("post_install", "-at_install", "standard")
class TestBrowserReporterAssets(HamsHttpCase):
    # Tests [@ANCHOR: hams_helpdesk:js_install]
    def test_the_reporter_is_in_the_site_and_back_office_bundles_and_nowhere_else(self):
        assets = get_manifest("hams_helpdesk")["assets"]
        files = ["hams_helpdesk/static/src/js/browser_error_reporter.js", "hams_helpdesk/static/src/js/browser_error_reporter_boot.js"]
        self.assertEqual(assets["web.assets_frontend"], files)
        self.assertEqual(assets["web.assets_backend"], files)
        for bundle, listed in assets.items():
            if bundle not in ("web.assets_frontend", "web.assets_backend", "web.assets_unit_tests"):
                self.assertFalse([name for name in listed if "browser_error_reporter" in name], bundle)
        self.assertNotIn("browser_error_reporter_boot.js", " ".join(assets["web.assets_unit_tests"]))

    def test_a_public_page_serves_the_reporter_from_its_own_bundle_with_no_inline_script(self):
        page = self.url_open("/").text
        # The page names the frontend bundle in two parts (the minimal one in a script tag, the lazy one for a later load),
        # so look at every frontend bundle the page names, not just the first.
        links = sorted(set(re.findall(r'/web/assets/[^"\'\s<>]+web\.assets_frontend[^"\'\s<>]*\.js', page)))
        self.assertTrue(links, "the page loads the frontend bundle from its own origin")
        bundles = {link: self.url_open(link).text for link in links}
        bundle = "\n".join(bundles.values())
        self.assertIn("@hams_helpdesk/js/browser_error_reporter", bundle, sorted(bundles))
        self.assertIn("/hams_helpdesk/browser_exception", bundle)
        # The reporter needs no inline script, so the strict policy is untouched.
        self.assertNotIn("browser_exception", re.sub(r"<script[^>]+src=[^>]*>", "", page))


@tagged("post_install", "-at_install", "hams_helpdesk", "standard")
class TestBrowserReporterHoot(HamsHttpCase):
    def test_browser_error_reporter_hoot_suite_passes(self):
        # Tests [@ANCHOR: hams_helpdesk:js_reporter_construct]
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=hams_helpdesk",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=lambda message: "[HOOT]" not in message,
        )
