# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""Cloudflare Zero Trust reusable Access policies (account level)."""
from .cloudflare_api import _make_request, _json_body

MAX_PAGES = 100


# [@ANCHOR: cloudflare:COMM_access_api_list_policies_checked]
def list_access_policies_checked(account_id, token):
    """Every reusable Access policy of the account as ("ok", [policy, ...]) or ("error", message).

    A planner must not take an unreadable list for an empty one: it would then create duplicates."""
    if not token or not account_id:
        return "error", "Missing credentials."
    policies, page = [], 1
    while page <= MAX_PAGES:
        endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/access/policies?page={page}&per_page=100"
        response = _make_request("GET", endpoint, token, "Cloudflare Access Policy List API failed", timeout=15)
        if response is None:
            return "error", "API Error"
        if response.status_code != 200:
            return "error", f"HTTP {response.status_code}"
        body = _json_body(response)
        result = body.get("result") if body else None
        if not isinstance(result, list):
            return "error", "unexpected response"
        policies.extend(result)
        pages = ((body.get("result_info") or {}).get("total_pages")) or 1
        if page >= pages:
            return "ok", policies
        page += 1
    return "error", "too many pages"


# [@ANCHOR: cloudflare:COMM_access_api_write_policy]
def write_access_policy(account_id, policy_id, payload, token):
    """Create (policy_id None) or update one Access policy. Returns (ok, message)."""
    if not token or not account_id:
        return False, "Missing credentials."
    base = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/access/policies"
    method, endpoint = ("PUT", f"{base}/{policy_id}") if policy_id else ("POST", base)
    response = _make_request(method, endpoint, token, "Cloudflare Access Policy write API failed", json=payload, timeout=15)
    if response and response.status_code == 200:
        return True, "written"
    return False, f"API Error{f' (HTTP {response.status_code})' if response else ''}"
