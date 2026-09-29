# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import hashlib
import hmac
import json
import logging

from odoo import _, http
from odoo.http import request

_logger = logging.getLogger(__name__)

# Admin-configured shared secret (see zero_sudo/models/security_utils.py's own
# _get_param_read_whitelist(), which must list this exact key -- its own comment there explains
# why the whitelist is the whole gate in hams_open, matching hams_helpdesk.ncmec_api_username/
# _password's own precedent). Unset by default: no real secret is configured for this deployment
# yet (GitHub's own webhook registration is a separate, later step -- see this branch's own task
# description), matching this codebase's established "unset by default, warn and no-op" shape.
_GITHUB_WEBHOOK_SECRET_PARAM = "hams_helpdesk.github_webhook_secret"

# A narrow, single-purpose service account (security/helpdesk_security.xml) -- create access on
# hams_helpdesk.ticket plus read access (via the zero_sudo whitelist above) to the shared secret,
# nothing more. Deliberately NOT user_helpdesk_service/user_ai_triage_service: this route is a
# public, unauthenticated HTTP endpoint reachable by anyone who can guess the URL, so it gets the
# narrowest possible privilege, matching group_ai_triage_external_service/
# group_pager_incident_creator's own "narrow, single-purpose service account, least privilege,
# don't reuse a broader one" precedent.
_GITHUB_WEBHOOK_SERVICE_XML_ID = "hams_helpdesk.user_github_webhook_service"

# dependabot_alert: only a NEW problem actually needs a ticket. 'dismissed'/'resolved'/
# 'auto_dismissed'/'fixed' are all the alert going away, not appearing -- see GitHub's own
# dependabot_alert webhook event documentation for the full action enum.
_DEPENDABOT_ACTIONABLE_ACTIONS = {"created", "reopened"}

# GitHub's own documented dependabot severity values (low/moderate/high/critical). 'critical' is
# deliberately never used for this ticket's own priority field -- hams_helpdesk.ticket's
# Critical priority ("3") is reserved for the NCMEC mandatory-reporting workflow (see
# helpdesk_ticket.py's own _CSAM_TICKET_TYPE/COMM_ncmec_force_priority) and must not be diluted
# by an ordinary (if serious) dependency vulnerability. 'high'/'critical' severities instead get
# this ticket model's own High priority ("2"); everything else (moderate/low/unknown) gets
# Medium ("1") -- a real, if not urgent, engineering signal, matching the task's own guidance.
_DEPENDABOT_HIGH_PRIORITY_SEVERITIES = {"high", "critical"}


def _verify_github_signature(raw_body, secret, signature_header):
    """Verifies GitHub's own ``X-Hub-Signature-256`` header: ``sha256=<hex hmac>`` of the raw
    request body, keyed by the shared webhook secret (GitHub's own documented scheme -- see
    https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries).

    ``raw_body`` MUST be the exact, unparsed request body bytes -- computing this against
    re-serialized JSON does not reproduce the same bytes GitHub itself signed (key order,
    whitespace, and unicode escaping are all not guaranteed to round-trip identically), and
    would make a genuine delivery fail verification.

    Uses ``hmac.compare_digest()`` rather than a plain ``==`` specifically to avoid a timing
    side-channel on the comparison (an attacker who can measure response latency could otherwise
    recover the expected signature one byte at a time) -- the same reasoning
    ses_webhook/controllers/webhook_api.py documents for its own (cert-based, not HMAC-based)
    verification.

    Returns False (never raises) for any missing/malformed input, matching
    ses_webhook's own "callers get a plain boolean gate" shape -- fails closed either way.
    """
    if not secret or not signature_header:
        return False
    if not signature_header.startswith("sha256="):
        return False
    provided_hex = signature_header[len("sha256="):]
    try:
        mac = hmac.new(secret.encode("utf-8"), msg=raw_body, digestmod=hashlib.sha256)
    except (TypeError, ValueError) as e:
        _logger.warning("GitHub webhook: could not compute HMAC for verification: %s", e)
        return False
    return hmac.compare_digest(mac.hexdigest(), provided_hex)


class GithubWebhookController(http.Controller):

    # [@ANCHOR: hams_helpdesk:COMM_github_webhook_receive]
    # Verified by [@ANCHOR: test_github_webhook_missing_secret_configured_is_denied]
    # Verified by [@ANCHOR: test_github_webhook_missing_signature_is_denied]
    # Verified by [@ANCHOR: test_github_webhook_invalid_signature_is_denied]
    # Verified by [@ANCHOR: test_github_webhook_tampered_body_with_valid_looking_signature_is_denied]
    # Verified by [@ANCHOR: test_github_webhook_dependabot_alert_created_creates_a_ticket]
    # Verified by [@ANCHOR: test_github_webhook_workflow_run_failure_creates_a_ticket]
    @http.route("/webhook/github", type="http", auth="public", methods=["POST"], csrf=False)
    def receive_github_webhook(self, **kwargs):
        """Receives GitHub's own webhook POSTs for two event types this deployment cares
        about: dependabot_alert (a new/reopened security-alert notification) and workflow_run
        (a completed, failed CI run) -- for both hams_com (private) and hams_open (public)
        repos, whichever one's own webhook is pointed at this same endpoint. Creates an
        ordinary hams_helpdesk.ticket through the model's own create() -- no separate
        notification/assignment logic here at all, so a new ticket gets the same on-duty
        routing (calendar.event.get_current_on_duty_admin(), see helpdesk_ticket.py's own
        create()) every other ticket already gets, automatically.

        GitHub's own webhook registration is a separate, later step (this route exists and is
        reachable before any real webhook is ever registered against it) -- see this branch's
        own task description for why that is deliberately out of scope here.
        """
        raw_body = request.httprequest.get_data()
        event_type = request.httprequest.headers.get("X-GitHub-Event", "")
        signature_header = request.httprequest.headers.get("X-Hub-Signature-256", "")

        # Bootstrap: resolve this route's own narrow service account, exactly the shape
        # ses_webhook/controllers/webhook_api.py established (a bare, unauthenticated
        # request.env call to obtain the uid, then with_user(svc_uid) for every operation that
        # follows) -- never the bare, unauthenticated request.env itself for the read below.
        svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
            _GITHUB_WEBHOOK_SERVICE_XML_ID
        )
        utils = request.env["zero_sudo.security.utils"].with_user(svc_uid)

        secret = utils._get_system_param(_GITHUB_WEBHOOK_SECRET_PARAM, "")
        if not secret:
            _logger.warning(
                "GitHub webhook denied: no shared secret configured (%s is unset).",
                _GITHUB_WEBHOOK_SECRET_PARAM,
            )
            return request.make_response("Forbidden", status=403)

        # Verified BEFORE any JSON parsing/dispatch below -- never process an unverified
        # payload, matching ses_webhook's own "signature gate runs before dispatch" ordering.
        if not _verify_github_signature(raw_body, secret, signature_header):
            _logger.warning(
                "GitHub webhook denied: signature verification failed for event %r.",
                event_type,
            )
            return request.make_response("Forbidden", status=403)

        try:
            payload = json.loads(raw_body.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            _logger.warning("GitHub webhook: invalid JSON payload for event %r.", event_type)
            return request.make_response("Bad Request", status=400)

        if not isinstance(payload, dict):
            # A JSON array/string/number is valid JSON but not a valid GitHub webhook payload
            # shape -- every .get() below assumes a dict, matching ses_webhook's own guard
            # against the identical "valid JSON, wrong top-level shape" case.
            return request.make_response("Bad Request", status=400)

        Ticket = request.env["hams_helpdesk.ticket"].with_user(svc_uid)

        if event_type == "dependabot_alert":
            self._handle_dependabot_alert(Ticket, payload)
        elif event_type == "workflow_run":
            self._handle_workflow_run(Ticket, payload)
        else:
            # Any other event type: GitHub must see a 2xx or it will keep retrying the
            # delivery indefinitely. Nothing to do for an event type this deployment doesn't
            # subscribe to (e.g. GitHub's own 'ping' sent when a webhook is first registered).
            _logger.info("GitHub webhook: ignoring unhandled event type %r.", event_type)

        return request.make_response("OK", status=200)

    # [@ANCHOR: hams_helpdesk:COMM_github_webhook_handle_dependabot_alert]
    def _handle_dependabot_alert(self, Ticket, payload):
        action = payload.get("action")
        if action not in _DEPENDABOT_ACTIONABLE_ACTIONS:
            # 'dismissed'/'resolved'/'auto_dismissed'/'fixed' etc. -- the alert going away, not
            # a new problem needing attention. See _DEPENDABOT_ACTIONABLE_ACTIONS's own comment.
            return

        alert = payload.get("alert") or {}
        html_url = alert.get("html_url")
        if not html_url:
            _logger.warning(
                "GitHub webhook: dependabot_alert payload missing alert.html_url; ignoring."
            )
            return

        repo_full_name = (payload.get("repository") or {}).get("full_name") or "unknown repository"
        # GitHub's own documented shape has the alert's severity under security_vulnerability;
        # security_advisory carries a duplicate copy on some payload versions -- checked as a
        # fallback only, never preferred over the more specific field.
        severity = (
            (alert.get("security_vulnerability") or {}).get("severity")
            or (alert.get("security_advisory") or {}).get("severity")
            or "unknown"
        )
        package_name = (
            ((alert.get("dependency") or {}).get("package") or {}).get("name")
            or "unknown package"
        )
        priority = (
            "2" if str(severity).lower() in _DEPENDABOT_HIGH_PRIORITY_SEVERITIES else "1"
        )

        name = _("Dependabot alert (%(severity)s): %(package)s in %(repo)s") % {
            "severity": severity,
            "package": package_name,
            "repo": repo_full_name,
        }
        description = (
            "<p>%s</p><ul><li>%s</li><li>%s</li><li>%s</li></ul>"
            % (
                _("A dependabot security alert (%s) was reported.") % action,
                _("Repository: %s") % repo_full_name,
                _("Severity: %s") % severity,
                _('Alert: <a href="%(url)s">%(url)s</a>') % {"url": html_url},
            )
        )
        Ticket._github_webhook_ticket_for(
            html_url,
            {
                "name": name,
                "description": description,
                "ticket_type": "general",
                "priority": priority,
            },
        )

    # [@ANCHOR: hams_helpdesk:COMM_github_webhook_handle_workflow_run]
    def _handle_workflow_run(self, Ticket, payload):
        if payload.get("action") != "completed":
            return
        workflow_run = payload.get("workflow_run") or {}
        if workflow_run.get("conclusion") != "failure":
            # 'cancelled'/'skipped'/'success'/'neutral'/'timed_out'/etc. -- only an actual
            # failure is the genuine engineering signal this ticket is for.
            return

        html_url = workflow_run.get("html_url")
        if not html_url:
            _logger.warning(
                "GitHub webhook: workflow_run payload missing workflow_run.html_url; ignoring."
            )
            return

        repo_full_name = (payload.get("repository") or {}).get("full_name") or "unknown repository"
        workflow_name = workflow_run.get("name") or "unknown workflow"
        branch = workflow_run.get("head_branch") or "unknown branch"
        commit_sha = workflow_run.get("head_sha") or "unknown commit"

        name = _("CI failure: %(workflow)s on %(repo)s") % {
            "workflow": workflow_name,
            "repo": repo_full_name,
        }
        description = (
            "<p>%s</p><ul><li>%s</li><li>%s</li><li>%s</li></ul>"
            % (
                _("A GitHub Actions workflow run failed."),
                _("Repository: %s") % repo_full_name,
                _("Branch/commit: %(branch)s @ %(commit)s")
                % {"branch": branch, "commit": commit_sha},
                _('Run: <a href="%(url)s">%(url)s</a>') % {"url": html_url},
            )
        )
        Ticket._github_webhook_ticket_for(
            html_url,
            {
                "name": name,
                "description": description,
                "ticket_type": "general",
                "priority": "1",
            },
        )
