# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""Tests for the GitHub webhook ticket-creation route (controllers/github_webhook_api.py):
GitHub's own dependabot_alert/workflow_run webhook POSTs turning into ordinary
hams_helpdesk.ticket records, for both hams_com and hams_open.

Signature verification here is plain HMAC-SHA256 over the raw request body, not a canonical
"string-to-sign" the production code and a test helper could independently disagree on the
shape of (contrast ses_webhook's own SNS signature verification, which really does have a
field-order-dependent string to build) -- there is no meaningful "independent" reimplementation
of hmac.new(...).hexdigest() itself, so _sign_github_payload below calls the same stdlib
primitive production does. What actually proves verification is real, not vacuous, is the
negative tests: missing secret, missing header, a wrong-key signature, and a tampered body kept
under an unchanged (now-stale) signature are all independently exercised and must each be
rejected with nothing created -- a mocked-out verification function could not distinguish any
of those cases from a valid one.
"""
import hashlib
import hmac
import json

from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase
from odoo.exceptions import UserError
from odoo.tests.common import tagged

_SECRET_PARAM = "hams_helpdesk.github_webhook_secret"
_TEST_SECRET = "test-github-webhook-shared-secret"


def _sign_github_payload(raw_body, secret=_TEST_SECRET):
    """Returns a real X-Hub-Signature-256 header value for `raw_body`, exactly the scheme
    GitHub itself uses (see _verify_github_signature's own docstring in github_webhook.py)."""
    mac = hmac.new(secret.encode("utf-8"), msg=raw_body, digestmod=hashlib.sha256)
    return f"sha256={mac.hexdigest()}"


def _dependabot_alert_payload(action="created", html_url=None, severity="high", package="requests"):
    html_url = html_url or f"https://github.com/BrucePerens/hams_open/security/dependabot/{action}-{severity}-{package}"
    return {
        "action": action,
        "alert": {
            "number": 1,
            "html_url": html_url,
            "state": "open" if action != "dismissed" else "dismissed",
            "dependency": {"package": {"ecosystem": "pip", "name": package}},
            "security_vulnerability": {"severity": severity},
            "security_advisory": {"severity": severity, "summary": "A vulnerability"},
        },
        "repository": {"full_name": "BrucePerens/hams_open"},
    }


def _workflow_run_payload(action="completed", conclusion="failure", html_url=None, run_id=42):
    html_url = html_url or f"https://github.com/BrucePerens/hams_open/actions/runs/{run_id}"
    return {
        "action": action,
        "workflow_run": {
            "id": run_id,
            "name": "CI",
            "html_url": html_url,
            "status": "completed",
            "conclusion": conclusion,
            "head_branch": "main",
            "head_sha": "abc123def456",
        },
        "repository": {"full_name": "BrucePerens/hams_com"},
    }


@tagged("post_install", "-at_install", "standard")
class TestGithubWebhookHttp(HamsHttpCase):
    """End-to-end HTTP-level tests: real requests through the real /webhook/github route,
    real HMAC verification, real hams_helpdesk.ticket rows -- nothing about the controller
    itself is mocked. Only the config-parameter read is patched, matching test_ncmec_report.py's
    own established `_get_system_param` patching convention (that method's own whitelist/
    mechanical-secret-block gate is real code neither this file nor that one bypasses)."""

    def setUp(self):
        super().setUp()
        self.Ticket = self.env["hams_helpdesk.ticket"]

    def _configure_secret(self, secret=_TEST_SECRET):
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils),
            "_get_system_param",
            lambda self, key, default=None: {_SECRET_PARAM: secret}.get(key, default),
            create=True,
        )

    def _post(self, event_type, payload, secret=_TEST_SECRET, sign=True, include_signature=True):
        raw_body = json.dumps(payload).encode("utf-8")
        headers = {"Content-Type": "application/json", "X-GitHub-Event": event_type}
        if include_signature:
            headers["X-Hub-Signature-256"] = (
                _sign_github_payload(raw_body, secret) if sign else "sha256=" + "0" * 64
            )
        return self.url_open("/webhook/github", data=raw_body, headers=headers)

    # ------------------------------------------------------------------
    # Signature / secret verification
    # ------------------------------------------------------------------

    def test_github_webhook_missing_secret_configured_is_denied(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_github_webhook_receive]
        # hams_helpdesk.github_webhook_secret is unset by default (never seeded by this
        # module's own data XML, on purpose) -- the real, no-patching-needed behavior.
        payload = _dependabot_alert_payload()
        raw_body = json.dumps(payload).encode("utf-8")
        response = self.url_open(
            "/webhook/github",
            data=raw_body,
            headers={
                "Content-Type": "application/json",
                "X-GitHub-Event": "dependabot_alert",
                "X-Hub-Signature-256": _sign_github_payload(raw_body),
            },
        )
        self.assertEqual(response.status_code, 403)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        )

    def test_github_webhook_missing_signature_is_denied(self):
        self._configure_secret()
        response = self._post(
            "dependabot_alert", _dependabot_alert_payload(), include_signature=False
        )
        self.assertEqual(response.status_code, 403)
        self.assertFalse(self.Ticket.search([]))

    def test_github_webhook_invalid_signature_is_denied(self):
        self._configure_secret()
        response = self._post("dependabot_alert", _dependabot_alert_payload(), sign=False)
        self.assertEqual(response.status_code, 403)
        self.assertFalse(self.Ticket.search([]))

    def test_github_webhook_wrong_key_signature_is_denied(self):
        # A signature computed with a DIFFERENT secret than the one configured -- distinct
        # from test_github_webhook_invalid_signature_is_denied's all-zero garbage signature,
        # proves this isn't just "any non-matching hex string is rejected" but a real
        # keyed-HMAC mismatch.
        self._configure_secret()
        payload = _dependabot_alert_payload()
        raw_body = json.dumps(payload).encode("utf-8")
        response = self.url_open(
            "/webhook/github",
            data=raw_body,
            headers={
                "Content-Type": "application/json",
                "X-GitHub-Event": "dependabot_alert",
                "X-Hub-Signature-256": _sign_github_payload(raw_body, secret="wrong-secret"),
            },
        )
        self.assertEqual(response.status_code, 403)
        self.assertFalse(self.Ticket.search([]))

    def test_github_webhook_tampered_body_with_stale_signature_is_denied(self):
        # Signs the ORIGINAL payload, then sends a DIFFERENT (tampered) body under that same,
        # now-stale signature header -- proves the signature is checked against the actual
        # bytes received, not merely present-and-well-formed.
        self._configure_secret()
        original = _dependabot_alert_payload(package="original-package")
        original_body = json.dumps(original).encode("utf-8")
        signature = _sign_github_payload(original_body)

        tampered = _dependabot_alert_payload(package="tampered-package")
        tampered_body = json.dumps(tampered).encode("utf-8")

        response = self.url_open(
            "/webhook/github",
            data=tampered_body,
            headers={
                "Content-Type": "application/json",
                "X-GitHub-Event": "dependabot_alert",
                "X-Hub-Signature-256": signature,
            },
        )
        self.assertEqual(response.status_code, 403)
        self.assertFalse(self.Ticket.search([]))

    def test_github_webhook_valid_signature_is_accepted(self):
        self._configure_secret()
        response = self._post("dependabot_alert", _dependabot_alert_payload())
        self.assertEqual(response.status_code, 200)
        self.assertEqual(len(self.Ticket.search([])), 1)

    # ------------------------------------------------------------------
    # dependabot_alert
    # ------------------------------------------------------------------

    def test_github_webhook_dependabot_alert_created_creates_a_ticket(self):
        # Tests [@ANCHOR: COMM_github_webhook_handle_dependabot_alert]
        self._configure_secret()
        payload = _dependabot_alert_payload(action="created", severity="high", package="lodash")
        response = self._post("dependabot_alert", payload)
        self.assertEqual(response.status_code, 200)

        ticket = self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        self.assertEqual(len(ticket), 1)
        self.assertEqual(ticket.ticket_type, "general")
        self.assertEqual(ticket.priority, "2", "high severity gets High priority.")
        self.assertIn("BrucePerens/hams_open", ticket.name)
        self.assertIn("lodash", ticket.name)
        self.assertIn(payload["alert"]["html_url"], ticket.description)

    def test_github_webhook_dependabot_alert_reopened_creates_a_ticket(self):
        self._configure_secret()
        payload = _dependabot_alert_payload(action="reopened", severity="moderate")
        response = self._post("dependabot_alert", payload)
        self.assertEqual(response.status_code, 200)
        self.assertEqual(
            len(self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])),
            1,
        )

    def test_github_webhook_dependabot_alert_low_severity_gets_normal_priority(self):
        self._configure_secret()
        payload = _dependabot_alert_payload(severity="low")
        self._post("dependabot_alert", payload)
        ticket = self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        self.assertEqual(ticket.priority, "1")

    def test_github_webhook_dependabot_alert_critical_severity_never_gets_critical_priority(self):
        # hams_helpdesk.ticket's own Critical priority ("3") is reserved for the NCMEC
        # mandatory-reporting workflow -- must not be diluted by a dependency vulnerability,
        # however severe.
        self._configure_secret()
        payload = _dependabot_alert_payload(severity="critical")
        self._post("dependabot_alert", payload)
        ticket = self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        self.assertEqual(ticket.priority, "2")

    def test_github_webhook_dependabot_alert_dismissed_creates_nothing(self):
        self._configure_secret()
        payload = _dependabot_alert_payload(action="dismissed")
        response = self._post("dependabot_alert", payload)
        self.assertEqual(response.status_code, 200)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        )

    def test_github_webhook_dependabot_alert_auto_dismissed_creates_nothing(self):
        self._configure_secret()
        payload = _dependabot_alert_payload(action="auto_dismissed")
        self._post("dependabot_alert", payload)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        )

    def test_github_webhook_dependabot_alert_fixed_creates_nothing(self):
        self._configure_secret()
        payload = _dependabot_alert_payload(action="fixed")
        self._post("dependabot_alert", payload)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        )

    # ------------------------------------------------------------------
    # workflow_run
    # ------------------------------------------------------------------

    def test_github_webhook_workflow_run_failure_creates_a_ticket(self):
        # Tests [@ANCHOR: COMM_github_webhook_handle_workflow_run]
        self._configure_secret()
        payload = _workflow_run_payload(conclusion="failure")
        response = self._post("workflow_run", payload)
        self.assertEqual(response.status_code, 200)

        ticket = self.Ticket.search(
            [("github_source_url", "=", payload["workflow_run"]["html_url"])]
        )
        self.assertEqual(len(ticket), 1)
        self.assertEqual(ticket.ticket_type, "general")
        self.assertEqual(ticket.priority, "1")
        self.assertIn("BrucePerens/hams_com", ticket.name)
        self.assertIn(payload["workflow_run"]["html_url"], ticket.description)
        self.assertIn("main", ticket.description)

    def test_github_webhook_workflow_run_not_completed_creates_nothing(self):
        self._configure_secret()
        payload = _workflow_run_payload(action="requested", conclusion=None)
        self._post("workflow_run", payload)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["workflow_run"]["html_url"])])
        )

    def test_github_webhook_workflow_run_success_creates_nothing(self):
        self._configure_secret()
        payload = _workflow_run_payload(conclusion="success")
        self._post("workflow_run", payload)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["workflow_run"]["html_url"])])
        )

    def test_github_webhook_workflow_run_cancelled_creates_nothing(self):
        self._configure_secret()
        payload = _workflow_run_payload(conclusion="cancelled")
        self._post("workflow_run", payload)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["workflow_run"]["html_url"])])
        )

    def test_github_webhook_workflow_run_skipped_creates_nothing(self):
        self._configure_secret()
        payload = _workflow_run_payload(conclusion="skipped")
        self._post("workflow_run", payload)
        self.assertFalse(
            self.Ticket.search([("github_source_url", "=", payload["workflow_run"]["html_url"])])
        )

    # ------------------------------------------------------------------
    # Unhandled event types / malformed payloads
    # ------------------------------------------------------------------

    def test_github_webhook_unhandled_event_type_returns_200_and_creates_nothing(self):
        self._configure_secret()
        response = self._post("ping", {"zen": "Keep it logically awesome."})
        self.assertEqual(response.status_code, 200)
        self.assertFalse(self.Ticket.search([]))

    def test_github_webhook_invalid_json_is_rejected(self):
        self._configure_secret()
        raw_body = b"not json"
        response = self.url_open(
            "/webhook/github",
            data=raw_body,
            headers={
                "Content-Type": "application/json",
                "X-GitHub-Event": "dependabot_alert",
                "X-Hub-Signature-256": _sign_github_payload(raw_body),
            },
        )
        self.assertEqual(response.status_code, 400)
        self.assertFalse(self.Ticket.search([]))

    def test_github_webhook_json_array_payload_is_rejected(self):
        self._configure_secret()
        raw_body = b"[1, 2, 3]"
        response = self.url_open(
            "/webhook/github",
            data=raw_body,
            headers={
                "Content-Type": "application/json",
                "X-GitHub-Event": "dependabot_alert",
                "X-Hub-Signature-256": _sign_github_payload(raw_body),
            },
        )
        self.assertEqual(response.status_code, 400)

    # ------------------------------------------------------------------
    # Dedup: real redelivery protection through the actual HTTP route
    # ------------------------------------------------------------------

    def test_github_webhook_redelivered_payload_does_not_duplicate_the_ticket(self):
        # GitHub retries a webhook delivery on any non-2xx response, and can occasionally
        # redeliver even an already-processed one -- a second, identical delivery must not
        # open a second ticket.
        self._configure_secret()
        payload = _dependabot_alert_payload()
        first = self._post("dependabot_alert", payload)
        second = self._post("dependabot_alert", payload)
        self.assertEqual(first.status_code, 200)
        self.assertEqual(second.status_code, 200)
        self.assertEqual(
            len(self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])),
            1,
            "A redelivered webhook payload must not create a duplicate ticket.",
        )

    def test_github_webhook_new_ticket_after_prior_one_closed(self):
        self._configure_secret()
        payload = _dependabot_alert_payload(action="created")
        self._post("dependabot_alert", payload)
        first = self.Ticket.search([("github_source_url", "=", payload["alert"]["html_url"])])
        self.assertEqual(len(first), 1)
        first.write({"stage": "closed"})

        reopened_payload = dict(payload, action="reopened")
        self._post("dependabot_alert", reopened_payload)
        all_tickets = self.Ticket.search(
            [("github_source_url", "=", payload["alert"]["html_url"])]
        )
        self.assertEqual(
            len(all_tickets),
            2,
            "A closed prior ticket must not absorb a genuinely new later delivery.",
        )


@tagged("post_install", "-at_install", "standard")
class TestGithubWebhookTicketFor(HamsTransactionCase):
    """Model-level tests for hams_helpdesk.ticket._github_webhook_ticket_for -- the dedup-aware
    creation entrypoint the controller itself calls (see test_ncmec_report.py's own
    _ncmec_report_ticket_for_recording tests for the matching sibling shape this mirrors)."""

    def test_github_webhook_ticket_for_creates_a_new_ticket(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_github_webhook_ticket_for]
        url = "https://github.com/BrucePerens/hams_open/actions/runs/1"
        ticket = self.env["hams_helpdesk.ticket"]._github_webhook_ticket_for(
            url, {"name": "CI failure", "ticket_type": "general", "priority": "1"}
        )
        self.assertEqual(ticket.github_source_url, url)
        self.assertEqual(ticket.ticket_type, "general")

    def test_github_webhook_ticket_for_skips_an_existing_open_ticket(self):
        url = "https://github.com/BrucePerens/hams_open/actions/runs/2"
        Ticket = self.env["hams_helpdesk.ticket"]
        first = Ticket._github_webhook_ticket_for(url, {"name": "First delivery"})
        second = Ticket._github_webhook_ticket_for(url, {"name": "Redelivery"})
        self.assertEqual(first, second)
        self.assertEqual(
            Ticket.search_count([("github_source_url", "=", url)]),
            1,
            "A redelivered payload for the same URL must not open a duplicate ticket.",
        )

    def test_github_webhook_ticket_for_reopens_after_prior_ticket_closed(self):
        url = "https://github.com/BrucePerens/hams_open/security/dependabot/3"
        Ticket = self.env["hams_helpdesk.ticket"]
        first = Ticket._github_webhook_ticket_for(url, {"name": "First flag"})
        first.write({"stage": "closed"})
        second = Ticket._github_webhook_ticket_for(url, {"name": "Reopened later"})
        self.assertNotEqual(
            first,
            second,
            "A closed prior ticket must not silently absorb a genuinely new later delivery.",
        )

    def test_github_webhook_ticket_for_rejects_github_source_url_in_vals(self):
        with self.assertRaises(UserError):
            self.env["hams_helpdesk.ticket"]._github_webhook_ticket_for(
                "https://github.com/BrucePerens/hams_open/actions/runs/4",
                {"name": "x", "github_source_url": "https://something-else"},
            )
