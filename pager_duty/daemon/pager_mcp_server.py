# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
PAGER_DUTY_MCP_AI_TRIAGE.md's own "Real build order" slice 1: an MCP
server exposing list_incidents/get_incident/add_incident_note against the
real, already-existing pager.incident model -- the genuinely non-
destructive tool set, safe to build without waiting on anything further.

This module is a thin RPC adapter, not a re-implementation of Odoo's own
logic: the real work (what fields to return, chatter formatting, the
AI-authored-note prefix) lives in incident.py's own mcp_list_incidents()/
mcp_get_incident_detail()/mcp_add_note() methods (see that file's own
pager_mcp_triage_tools anchor comment). This server just calls them over
the same JSON-2 API generalized_monitor.py's own OdooClient already uses,
credentialed as the narrowly-scoped pager_duty.user_pager_mcp_triage_service
account (read-only on pager.incident, see security.xml) -- deliberately
NOT the broader pager_service_internal account generalized_monitor.py
itself runs as, since this server should never be able to write a
pager.incident field directly, only call the three specific methods that
internally elevate for exactly the one thing each needs to do.

Deliberately does NOT expose set_incident_status (acknowledge/resolve) --
see PAGER_DUTY_MCP_AI_TRIAGE.md's own slice 1a for why that one needs its
own explicit go/no-go from Bruce before it's ever built: acknowledging or
resolving an incident is exactly the action that silences a real page.
"""

import argparse
import json
import logging
import os
import urllib.error
import urllib.request

from mcp.server.fastmcp import FastMCP

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - [%(levelname)s] - %(message)s"
)
logger = logging.getLogger("pager_mcp_server")

mcp = FastMCP("PagerDutyTriage")


class OdooClient:
    """Same JSON-2 API shape as generalized_monitor.py's own OdooClient --
    duplicated rather than imported, since that module is a standalone
    daemon script (not a package this one can cleanly import from) and the
    two clients carry genuinely different, independently-auditable
    credential scopes (broad pager_service_internal there, narrow
    pager_mcp_triage_service here). A record-bound method call passes
    `ids=[...]` as a kwarg alongside the method's own arguments -- the same
    convention generalized_monitor.py's own `write`/`rpc_ensure_executable`
    calls already use; an `@api.model` call omits it."""

    def __init__(self, url, db, api_key):
        self.url = url.rstrip("/")
        self.db = db
        self.headers = {
            "Authorization": f"bearer {api_key}",
            "X-Odoo-Database": db,
            "Content-Type": "application/json",
            "User-Agent": "Pager-MCP-Triage/1.0",
        }

    def execute(self, model, method, **kwargs):
        req = urllib.request.Request(
            f"{self.url}/json/2/{model}/{method}",
            data=json.dumps(kwargs).encode("utf-8"),
            headers=self.headers,
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=15) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            err_body = e.read().decode("utf-8")
            raise RuntimeError(
                f"Odoo JSON-2 API error {e.code} calling {model}.{method}: {err_body}"
            )


def _get_client():
    url = os.environ.get("ODOO_URL") or "http://odoo:8069"
    db = os.environ.get("ODOO_DB") or "odoo"
    # burn-ignore-env: a real Odoo API key for
    # pager_duty.user_pager_mcp_triage_service, not a plaintext account
    # password -- see hams-secrets-directory-convention (~/.secrets, never
    # a plain env default) for how this gets provisioned in a real
    # deployment. No fallback value: a missing key must fail loudly, not
    # silently degrade to an unauthenticated or wrong-identity call.
    api_key = os.environ.get("PAGER_MCP_API_KEY")
    if not api_key:
        raise RuntimeError(
            "PAGER_MCP_API_KEY is not set -- this must be a real Odoo API key "
            "issued to pager_duty.user_pager_mcp_triage_service."
        )
    return OdooClient(url, db, api_key)


@mcp.tool()
def list_incidents(status: str = None, severity: str = None, limit: int = 50) -> str:
    """List pager.incident records, optionally filtered by status
    (open/acknowledged/resolved) and/or severity (low/medium/high/critical),
    newest first. Read-only."""
    client = _get_client()
    result = client.execute(
        "pager.incident",
        "mcp_list_incidents",
        status=status,
        severity=severity,
        limit=limit,
    )
    return json.dumps(result)


@mcp.tool()
def get_incident(incident_id: int) -> str:
    """Full detail for one pager.incident: source, severity, description,
    status, occurrence_count, and its chatter history (prior notes/advice
    already posted, including by this same tool on a previous pass)."""
    client = _get_client()
    result = client.execute(
        "pager.incident", "mcp_get_incident_detail", ids=[incident_id]
    )
    return json.dumps(result)


@mcp.tool()
def add_incident_note(incident_id: int, text: str) -> str:
    """Posts `text` to an incident's own chatter, tagged as AI-authored
    ("🤖 AI Triage: ..."). Cannot change the incident's own status or any
    other field -- see this module's own doc comment on why
    set_incident_status is a separate, not-yet-built tool."""
    client = _get_client()
    client.execute("pager.incident", "mcp_add_note", ids=[incident_id], text=text)
    return json.dumps({"ok": True, "incident_id": incident_id})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--transport",
        default="stdio",
        choices=["stdio", "sse", "streamable-http"],
        help="Same transport choices mcp_watchdog.py's own CLI already exposes.",
    )
    args = parser.parse_args()
    mcp.run(transport=args.transport)
