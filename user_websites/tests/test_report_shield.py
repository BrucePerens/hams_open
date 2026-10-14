# SPDX-License-Identifier: AGPL-3.0-or-later
"""The report-violation form behind the form shield (form_shield module)."""
import lxml.html
import redis

import odoo
from odoo.addons.form_shield.tests.test_support import (
    reset_shield_state,
    shield_fields,
    shield_response,
)
from odoo.addons.user_websites.controllers import main as uw_main
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged

FORM = "report_violation"
FORGED = "v1.pow.1.2.3.aa.bb.cc~1"


@tagged("post_install", "-at_install")
class TestReportShield(HamsHttpCase):
    def setUp(self):
        super().setUp()
        reset_shield_state(self.env)
        self.owner = self.env["res.users"].create(
            {
                "name": "Shield Owner",
                "login": "shieldowner",
                "email": "shieldowner@example.com",
                "website_slug": "shieldowner",
                "group_ids": [
                    (6, 0, [self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id])
                ],
            }
        )
        self.Report = self.env["content.violation.report"]
        self.Counter = self.env["form.shield.rejection"]
        self.authenticate(None, None)
        self.token = odoo.http.Request.csrf_token(self)

    def reports(self, email):
        return self.Report.search([("reported_by_email", "=", email)])

    def counted(self, reason):
        rows = self.Counter.search([("form_id", "=", FORM), ("reason", "=", reason)])
        return sum(rows.mapped("count"))

    def post(self, email="reporter@example.com", url=None, **data):
        body = {
            "csrf_token": self.token,
            "url": url if url is not None else f"/{self.owner.website_slug}",
            "description": "This page advertises something it should not.",
            "email": email,
        }
        body.update(data)
        return self.url_open(
            "/website/report_violation", data=body, method="POST", allow_redirects=False
        )

    # [@ANCHOR: user_websites:test_report_violation_shield_outcomes]
    def test_report_violation_shield_outcomes(self):
        # Tests [@ANCHOR: user_websites:report_violation_shield]
        # Tests [@ANCHOR: user_websites:render_report_page]
        # Tests [@ANCHOR: user_websites:report_refusal]
        # Tests [@ANCHOR: form_shield:refusal_response]
        # Tests [@ANCHOR: form_shield:refused_page]
        email = "outcomes@example.com"
        # No answer at all: a page with the question, what was typed kept, nothing stored.
        page = self.post(email=email)
        self.assertEqual(page.status_code, 200)
        tree = lxml.html.fromstring(page.content)
        self.assertTrue(tree.xpath("//label[@for='shield_answer']"))
        self.assertIn("advertises something", tree.xpath("//textarea[@name='description']")[0].text)
        self.assertFalse(self.reports(email))
        # A program: forged answer, either honeypot, an answer for another form. All look like
        # success, and none is stored.
        wrong_form = shield_fields(self.env, "contact")
        for label, extra in (
            ("forged", {"shield_response": FORGED}),
            ("honeypot", {**shield_fields(self.env, FORM), "fs_homepage": "x"}),
            ("old honeypot", {**shield_fields(self.env, FORM), "website_honeypot": "x"}),
            ("another form", wrong_form),
        ):
            response = self.post(email=email, **extra)
            self.assertEqual(response.status_code, 303, label)
            self.assertIn("report_submitted=1", response.headers["Location"], label)
        self.assertFalse(self.reports(email))
        for reason in ("bad_solution", "honeypot"):
            self.assertGreaterEqual(self.counted(reason), 1, reason)
        # A person who can retry is told why, and sent back to the page.
        for label, age, address, reason in (
            ("expired", 8000, None, "expired"),
            ("too fast", 0, None, "too_fast"),
            ("moved networks", 30, "198.51.100.7", "ip_changed"),
        ):
            fields_ = {"shield_response": shield_response(
                self.env, FORM, **({"address": address} if address else {}), age=age)}
            response = self.post(email=email, **fields_)
            self.assertEqual(response.status_code, 303, label)
            self.assertIn("form_shield_error=" + reason, response.headers["Location"], label)
            self.assertTrue(response.headers["Location"].startswith("/shieldowner"), label)
        self.assertFalse(self.reports(email))
        # Solved: stored once; the same answer again looks like success but stores nothing.
        answer = shield_fields(self.env, FORM)
        accepted = self.post(email=email, **answer)
        self.assertIn("report_submitted=1", accepted.headers["Location"])
        self.assertEqual(len(self.reports(email)), 1)
        replay = self.post(email=email, **answer)
        self.assertIn("report_submitted=1", replay.headers["Location"])
        self.assertEqual(len(self.reports(email)), 1)
        self.assertGreaterEqual(self.counted("replay"), 1)
        self.assertGreaterEqual(self.counted("accepted"), 1)

    def test_report_violation_limits_and_unavailable_store(self):
        # Tests [@ANCHOR: user_websites:report_limits]
        self.safe_patch_object(
            uw_main, "REPORT_LIMITS", (("subject", 1, 3600), ("global", 100, 3600))
        )
        first = self.post(email="limit@example.com", **shield_fields(self.env, FORM))
        self.assertIn("report_submitted=1", first.headers["Location"])
        second = self.post(email="limit@example.com", **shield_fields(self.env, FORM))
        self.assertEqual(second.status_code, 429)
        self.assertIn("Too many reports", second.text)
        self.assertEqual(len(self.reports("limit@example.com")), 1)
        # The same page reported from too many networks: the per-page limit.
        reset_shield_state(self.env)
        self.safe_patch_object(
            uw_main, "REPORT_LIMITS", (("target", 1, 86400), ("global", 100, 3600))
        )
        self.post(email="a@example.com", **shield_fields(self.env, FORM))
        other_address = "198.51.100.44"
        other_network = self.url_open(
            "/website/report_violation",
            data={
                "csrf_token": self.token,
                "url": "/shieldowner",
                "description": "Reported again from another network.",
                "email": "b@example.com",
                **shield_fields(self.env, FORM, other_address),
            },
            headers={"CF-Connecting-IP": other_address},
            method="POST",
            allow_redirects=False,
        )
        self.assertEqual(other_network.status_code, 429)
        # Redis down: the page says so, with 503; nothing is stored and nothing skipped.
        def broken(model):
            raise redis.exceptions.ConnectionError("down")

        self.safe_patch_object(type(self.env["form.shield"]), "_redis", broken)
        down = self.post(email="down@example.com", **shield_fields(self.env, FORM))
        self.assertEqual(down.status_code, 503)
        self.assertFalse(self.reports("down@example.com"))

    # [@ANCHOR: form_shield:test_question_form_page_renders_accessible_question]
    def test_question_form_page_renders_accessible_question(self):
        # Tests [@ANCHOR: user_websites:report_violation_page]
        # Tests [@ANCHOR: form_shield:question_fields_template]
        page = self.url_open("/website/report_violation?url=/shieldowner")
        self.assertEqual(page.status_code, 200)
        self.assertEqual(page.headers["Cache-Control"], "no-store")
        tree = lxml.html.fromstring(page.content)
        label = tree.xpath("//label[@for='shield_answer']")[0]
        self.assertTrue(label.text_content().startswith("What is"))
        answer = tree.xpath("//input[@id='shield_answer']")[0]
        self.assertEqual(answer.get("name"), "shield_answer")
        self.assertIsNotNone(answer.get("required"))
        self.assertEqual(answer.get("inputmode"), "numeric")
        self.assertTrue(answer.get("aria-describedby"))
        self.assertTrue(tree.xpath("//div[@id='%s']" % answer.get("aria-describedby")))
        self.assertTrue(tree.xpath("//textarea[@id='description']"))
        self.assertTrue(tree.xpath("//label[@for='description']"))
        self.assertTrue(tree.xpath("//input[@name='shield_challenge'][string-length(@value) > 20]"))
        trap = tree.xpath("//input[@name='fs_homepage']")[0]
        self.assertEqual(trap.get("tabindex"), "-1")
        self.assertNotIn("display:none", (trap.getparent().getparent().get("style") or "").replace(" ", ""))
        # Answering it files the report; a wrong answer sends the visitor back to try again.
        question = shield_response(self.env, FORM, kind="q")
        challenge, _, right = question.partition("~")
        email = "question@example.com"
        wrong = self.post(email=email, shield_challenge=challenge, shield_answer="0")
        self.assertIn("form_shield_error=wrong_answer", wrong.headers["Location"])
        question = shield_response(self.env, FORM, kind="q")
        challenge, _, right = question.partition("~")
        done = self.post(email=email, shield_challenge=challenge, shield_answer=right)
        self.assertIn("report_submitted=1", done.headers["Location"])
        self.assertEqual(len(self.reports(email)), 1)

    # [@ANCHOR: user_websites:test_normalize_report_target]
    def test_normalize_report_target(self):
        # Tests [@ANCHOR: user_websites:normalize_report_target]
        # Tests [@ANCHOR: user_websites:reported_path]
        # Tests [@ANCHOR: user_websites:bare_host]
        # Tests [@ANCHOR: user_websites:report_own_site_hosts]
        # Many posts from one network: lift the per-network limit, which is not under test here.
        self.safe_patch_object(
            uw_main, "REPORT_LIMITS", (("subject", 1000, 3600), ("global", 1000, 3600))
        )
        base = self.base_url()
        host = base.split("://", 1)[1]
        self.env["website"].search([], limit=1).domain = "https://shield-test.example"
        good = {
            "/shieldowner?x=1#section": f"{base}/shieldowner?x=1",
            f"{base}/shieldowner/blog": f"{base}/shieldowner/blog",
            base.replace("http://", "https://") + "/shieldowner": base.replace("http://", "https://") + "/shieldowner",
            f"http://www.{host}/shieldowner": f"http://www.{host}/shieldowner",
            "https://shield-test.example/shieldowner": "https://shield-test.example/shieldowner",
            "http://WWW.Shield-Test.example/a": "http://www.shield-test.example/a",
        }
        for number, (raw, stored) in enumerate(good.items()):
            email = f"good{number}@example.com"
            response = self.post(email=email, url=raw, **shield_fields(self.env, FORM))
            self.assertIn("report_submitted=1", response.headers["Location"], raw)
            self.assertEqual(self.reports(email).target_url, stored, raw)
        bad = (
            "https://evil.example/shieldowner",
            "//evil.example/shieldowner",
            "javascript:alert(1)",
            "ftp://" + host + "/shieldowner",
            f"http://user:secret@{host}/shieldowner",
            "http://shield-test.example.evil.example/x",
            "shieldowner",
            "/shield\nowner",
            "/" + "a" * 2100,
        )
        for number, raw in enumerate(bad):
            email = f"bad{number}@example.com"
            response = self.post(email=email, url=raw, **shield_fields(self.env, FORM))
            location = response.headers["Location"]
            self.assertIn("report_error=foreign_target", location, raw)
            self.assertTrue(location.startswith("/"), raw)
            self.assertFalse(location.startswith("//"), raw)
            self.assertFalse(self.reports(email), raw)
        # An accepted host with an extra slash must not turn the redirect protocol-relative.
        sneaky = self.post(
            url=f"{base}//evil.example/x", description="short", **shield_fields(self.env, FORM)
        )
        self.assertFalse(sneaky.headers["Location"].startswith("//"))
        self.assertGreaterEqual(self.counted("foreign_target"), len(bad))

    # [@ANCHOR: user_websites:test_report_problem_reasons]
    def test_report_problem_reasons(self):
        # Tests [@ANCHOR: user_websites:report_problem]
        # Tests [@ANCHOR: user_websites:local_part_ok]
        # Tests [@ANCHOR: form_shield:refuse]
        problem = uw_main.report_problem
        target = "http://x.example/a"
        ok_text = "This page advertises something it should not."
        self.assertIsNone(problem(target, ok_text, "r@example.com", False))
        self.assertEqual(problem(None, ok_text, "r@example.com", False), "foreign_target")
        self.assertEqual(problem(target, "Spam!!", "r@example.com", False), "short_description")
        self.assertEqual(problem(target, "   Test   ", "r@example.com", True), "short_description")
        # The random string that arrived as report 7 on 2026-10-12.
        self.assertEqual(
            problem(target, "uZ8rlF0ykP7dzW5kxD3zdJ6ng_soob", "r@example.com", False), "gibberish"
        )
        self.assertEqual(
            problem(target, "uZ8rlF0ykP7dzW5kxD3zdJ6ng_soob", "r@example.com", True), "gibberish"
        )
        # One long word is not gibberish if it is plainly a word (no digits and mixed case).
        self.assertIsNone(
            problem(target, "Antidisestablishmentarianism is here", "r@example.com", False)
        )
        links = "see http://a.example http://b.example http://c.example please"
        self.assertEqual(problem(target, links, "r@example.com", False), "too_many_urls")
        self.assertIsNone(problem(target, links, "r@example.com", True))
        two = "see http://a.example and www.b.example for details"
        self.assertIsNone(problem(target, two, "r@example.com", False))
        # Report 7's address, and the other ways an address is wrong.
        for bad in ("me.g.astop.l.ay.@gmail.com", "not-an-email", "", ".a@example.com", "a..b@example.com"):
            self.assertEqual(problem(target, ok_text, bad, False), "bad_email", repr(bad))
        self.assertIsNone(problem(target, ok_text, "", True))
        # Over HTTP the refusal is told to the visitor and counted by reason.
        before = self.counted("bad_email")
        response = self.post(email="me.g.astop.l.ay.@gmail.com", **shield_fields(self.env, FORM))
        self.assertIn("report_error=bad_email", response.headers["Location"])
        self.assertEqual(self.counted("bad_email"), before + 1)
        self.assertFalse(self.reports("me.g.astop.l.ay.@gmail.com"))

    def test_member_reports_without_a_challenge(self):
        # Tests [@ANCHOR: user_websites:report_violation_shield]
        self.owner.password = "shieldowner-pass"
        self.authenticate(self.owner.login, "shieldowner-pass")
        self.token = odoo.http.Request.csrf_token(self)
        response = self.post(email="", description="A member reports without a challenge.")
        self.assertIn("report_submitted=1", response.headers["Location"])
        stored = self.Report.search([("description", "=", "A member reports without a challenge.")])
        self.assertEqual(stored.reported_by_user_id, self.owner)
        trap = self.post(email="", description="A member who fills the honeypot.", fs_homepage="x")
        self.assertIn("report_submitted=1", trap.headers["Location"])
        self.assertFalse(self.Report.search([("description", "=", "A member who fills the honeypot.")]))
