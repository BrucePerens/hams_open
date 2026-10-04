# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import re
import requests
import logging
import base64
import secrets
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry

_logger = logging.getLogger(__name__)


# [@ANCHOR: cloudflare:COMM_handle_api_error]
def _handle_api_error(context_msg, exception):
    if "External requests verboten" in str(exception):
        _logger.info("%s (Disabled in tests): %s", context_msg, exception)
    else:
        _logger.error("%s: %s", context_msg, exception)


# Methods that are safe to replay because repeating them cannot change the
# result beyond what the first attempt already did. Everything else -- POST and
# PATCH against this API -- may have been applied by Cloudflare before the error
# reached us, so replaying it can create a duplicate DNS record, a duplicate
# firewall access rule, or a duplicate tunnel route.
IDEMPOTENT_METHODS = frozenset({"HEAD", "GET", "OPTIONS", "PUT", "DELETE"})

# The statuses worth retrying for an idempotent method: rate limiting plus the
# transient server-side failures.
IDEMPOTENT_RETRY_STATUSES = [429, 500, 502, 503, 504]
# # Verified by [@ANCHOR: test_cf_post_502_is_not_resent]

# The only status worth retrying for a non-idempotent method. A 429 is refused
# by Cloudflare's rate limiter before the request is applied, so replaying it
# cannot duplicate anything; a 5xx carries no such guarantee.
NON_IDEMPOTENT_RETRY_STATUSES = frozenset({429})
# # Verified by [@ANCHOR: test_cf_post_429_is_retried]


class IdempotencyAwareRetry(Retry):
    """Retry 5xx only for idempotent methods; retry POST/PATCH only on 429.

    urllib3 decides retries from one ``status_forcelist`` shared by every
    method, gated by ``allowed_methods``. That is too coarse here: we want the
    full transient-failure list for GET and friends, and only a rate-limit
    refusal for POST and PATCH. Overriding ``is_retry`` is what splits the two,
    and ``Retry.new()`` constructs ``type(self)``, so the subclass survives
    every retry in a sequence rather than degrading to the base class.

    ``allowed_methods`` deliberately stays restricted to the idempotent set so
    that urllib3's own read-error handling keeps refusing to replay a POST whose
    response was lost in transit -- that request may well have been applied.
    A connection error is exempt in urllib3 regardless of method, correctly:
    the request was never delivered.
    """

    # [@ANCHOR: cloudflare:COMM_idempotency_aware_retry]
    def is_retry(self, method, status_code, has_retry_after=False):
        # # Verified by [@ANCHOR: test_cf_retry_policy_excludes_non_idempotent_methods]
        if method.upper() in IDEMPOTENT_METHODS:
            return super().is_retry(method, status_code, has_retry_after)
        return status_code in NON_IDEMPOTENT_RETRY_STATUSES


session = requests.Session()
retry_strategy = IdempotencyAwareRetry(
    total=3,
    status_forcelist=IDEMPOTENT_RETRY_STATUSES,
    allowed_methods=sorted(IDEMPOTENT_METHODS),
    # Honour Retry-After when Cloudflare sends one; this is urllib3's default
    # and is named explicitly because the 429-on-POST path depends on it.
    respect_retry_after_header=True,
)
adapter = HTTPAdapter(max_retries=retry_strategy)
session.mount("https://", adapter)
session.mount("http://", adapter)


# [@ANCHOR: cloudflare:COMM_make_request]
def _make_request(method, endpoint, token, error_msg, **kwargs):
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
    if "headers" in kwargs:
        headers.update(kwargs.pop("headers"))

    timeout = kwargs.pop("timeout", 15)

    # audit-ignore-outbound-fetch: every caller in this file builds `endpoint` from the literal https://api.cloudflare.com/client/v4
    # base (or the fixed turnstile siteverify URL) and only interpolates zone, account, tunnel and record ids into the path.
    try:
        if method.upper() == "GET":
            response = session.get(endpoint, headers=headers, timeout=timeout, **kwargs)  # audit-ignore-outbound-fetch
        elif method.upper() == "POST":
            response = session.post(  # audit-ignore-outbound-fetch
                endpoint, headers=headers, timeout=timeout, **kwargs
            )
        elif method.upper() == "PUT":
            response = session.put(endpoint, headers=headers, timeout=timeout, **kwargs)  # audit-ignore-outbound-fetch
        elif method.upper() == "PATCH":
            response = session.patch(  # audit-ignore-outbound-fetch
                endpoint, headers=headers, timeout=timeout, **kwargs
            )
        elif method.upper() == "DELETE":
            response = session.delete(  # audit-ignore-outbound-fetch
                endpoint, headers=headers, timeout=timeout, **kwargs
            )
        else:
            raise ValueError(f"Unsupported HTTP method: {method}")

        if response.status_code == 404:
            return response

        response.raise_for_status()
        return response
    except requests.exceptions.RequestException as e:
        _handle_api_error(error_msg, e)
        return None


def purge_urls(urls, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_purge_urls_api]
    if not token or not zone_id:
        return False
    if not urls:
        return True

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache"
    success = True
    for i in range(0, len(urls), 30):
        chunk = urls[i : i + 30]
        payload = {"files": chunk}
        response = _make_request(
            "POST",
            endpoint,
            token,
            "Cloudflare URL purge API failed for chunk",
            json=payload,
            timeout=10,
        )
        if not response or response.status_code != 200:
            success = False
    return success


# [@ANCHOR: cloudflare:COMM_purge_tags]
def purge_tags(tags, token, zone_id):
    if not token or not zone_id:
        return False
    if not tags:
        return True

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache"
    success = True
    for i in range(0, len(tags), 30):
        chunk = tags[i : i + 30]
        payload = {"tags": chunk}
        response = _make_request(
            "POST",
            endpoint,
            token,
            "Cloudflare Tag purge API failed for chunk",
            json=payload,
            timeout=10,
        )
        if not response or response.status_code != 200:
            success = False
    return success


# # Verified by [@ANCHOR: test_cf_ban_ip_api]
def ban_ip(ip_address, mode, notes, token, zone_id):
    # # Verified by [@ANCHOR: test_cf_ban_ip_api]
    if not token or not zone_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/firewall/access_rules/rules"
    payload = {
        "mode": mode,
        "configuration": {"target": "ip", "value": ip_address},
        "notes": notes,
    }

    response = _make_request(
        "POST",
        endpoint,
        token,
        "Cloudflare WAF IP Ban API failed",
        json=payload,
        timeout=10,
    )
    if response and response.status_code == 200:
        rule_id = response.json().get("result", {}).get("id")
        return True, rule_id
    return False, "API Error"


def unban_ip(rule_id, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_02_cf_action_lift_ban]
    if not token or not zone_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/firewall/access_rules/rules/{rule_id}"
    response = _make_request(
        "DELETE", endpoint, token, "Cloudflare WAF IP Unban API failed", timeout=10
    )
    if response and response.status_code == 200:
        return True, "Success"
    return False, "API Error"


def verify_turnstile(token, remote_ip, secret):
    # # Verified by [@ANCHOR: COMM_test_cf_turnstile_verify]
    if not secret or not token:
        return False

    endpoint = "https://challenges.cloudflare.com/turnstile/v0/siteverify"
    data = {"secret": secret, "response": token}
    if remote_ip:
        data["remoteip"] = remote_ip

    try:
        response = session.post(endpoint, data=data, timeout=10)  # audit-ignore-outbound-fetch
        response.raise_for_status()
        return response.json().get("success", False)
    except requests.exceptions.RequestException as e:
        _handle_api_error("Cloudflare Turnstile verification failed", e)
        return False


def get_zone_ruleset(phase, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_03_cf_action_pull_waf_rules]
    if not token or not zone_id:
        return None

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/rulesets/phases/{phase}/entrypoint"
    response = _make_request(
        "GET", endpoint, token, "Cloudflare Ruleset Fetch API failed", timeout=15
    )
    if response:
        if response.status_code == 404:
            return None
        return response.json().get("result")
    return None


def update_zone_ruleset(ruleset_id, payload, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_04_cf_action_push_waf_rules]
    if not token or not zone_id:
        return False, "Missing credentials."

    endpoint = (
        f"https://api.cloudflare.com/client/v4/zones/{zone_id}/rulesets/{ruleset_id}"
    )
    response = _make_request(
        "PUT",
        endpoint,
        token,
        "Cloudflare Ruleset Update API failed",
        json=payload,
        timeout=15,
    )
    if response and response.status_code == 200:
        return True, "Ruleset updated successfully."
    return False, "API Error"


def create_zone_ruleset(payload, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_04_cf_action_push_waf_rules]
    if not token or not zone_id:
        return False, "Missing credentials."

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/rulesets"
    response = _make_request(
        "POST",
        endpoint,
        token,
        "Cloudflare Ruleset Create API failed",
        json=payload,
        timeout=15,
    )
    if response and response.status_code == 200:
        return True, "Ruleset created successfully."
    return False, "API Error"


def create_cfd_tunnel(account_id, token, tunnel_name):
    # # Verified by [@ANCHOR: COMM_test_cf_tunnel_setup]
    if not token or not account_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel"
    secret = base64.b64encode(secrets.token_bytes(32)).decode("utf-8")
    payload = {"name": tunnel_name, "tunnel_secret": secret}

    response = _make_request(
        "POST",
        endpoint,
        token,
        "Cloudflare Tunnel Create API failed",
        json=payload,
        timeout=15,
    )
    if response and response.status_code == 200:
        return True, response.json().get("result", {}).get("id")
    return False, "API Error"


def get_cfd_tunnel_token(account_id, token, tunnel_id):
    # # Verified by [@ANCHOR: COMM_test_cf_tunnel_setup]
    if not token or not account_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/token"
    response = _make_request(
        "GET", endpoint, token, "Cloudflare Tunnel Token API failed", timeout=15
    )
    if response and response.status_code == 200:
        return True, response.json().get("result", "")
    return False, "API Error"


# [@ANCHOR: cloudflare:COMM_update_cfd_tunnel_configuration]
def update_cfd_tunnel_configuration(account_id, token, tunnel_id, payload):
    if not token or not account_id or not tunnel_id:
        return False, "Missing credentials or tunnel ID"

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations"
    response = _make_request(
        "PUT",
        endpoint,
        token,
        "Cloudflare Update Tunnel Configuration API failed",
        json=payload,
        timeout=15,
    )
    if response and response.status_code == 200:
        return True, "Configuration updated successfully."
    return False, "API Error"

def purge_everything(token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_purge_everything_logic]
    if not token or not zone_id:
        return False

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache"
    payload = {"purge_everything": True}
    response = _make_request(
        "POST",
        endpoint,
        token,
        "Cloudflare Purge Everything API failed",
        json=payload,
        timeout=10,
    )
    if response and response.status_code == 200:
        return True
    return False


def get_zone_settings(token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_04_zone_settings_tour]
    if not token or not zone_id:
        return None

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/settings"
    response = _make_request(
        "GET", endpoint, token, "Cloudflare Get Zone Settings API failed", timeout=10
    )
    if response:
        if response.status_code == 404:
            return None
        return response.json().get("result")
    return None


def update_zone_setting(setting_name, value, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_04_zone_settings_tour]
    if not token or not zone_id:
        return False, "Missing credentials"

    endpoint = (
        f"https://api.cloudflare.com/client/v4/zones/{zone_id}/settings/{setting_name}"
    )
    payload = {"value": value}
    response = _make_request(
        "PATCH",
        endpoint,
        token,
        "Cloudflare Update Zone Setting API failed",
        json=payload,
        timeout=10,
    )
    if response and response.status_code == 200:
        return True, "Setting updated successfully."
    return False, "API Error"


def list_cfd_tunnels(account_id, token):
    # # Verified by [@ANCHOR: COMM_test_cf_sync_tunnels]
    if not token or not account_id:
        return []

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel"
    response = _make_request(
        "GET", endpoint, token, "Cloudflare List Tunnels API failed", timeout=15
    )
    if response and response.status_code == 200:
        return response.json().get("result", [])
    return []


def delete_cfd_tunnel(account_id, token, tunnel_id):
    # # Verified by [@ANCHOR: COMM_test_cf_delete_tunnel]
    if not token or not account_id or not tunnel_id:
        return False, "Missing credentials or tunnel ID"

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}"
    response = _make_request(
        "DELETE", endpoint, token, "Cloudflare Delete Tunnel API failed", timeout=15
    )
    if response and response.status_code == 200:
        return True, "Tunnel deleted successfully."
    return False, "API Error"


def create_custom_hostname(hostname, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_03_tunnel_setup]
    if not token or not zone_id or not hostname:
        return False, "Missing credentials or hostname"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/custom_hostnames"
    payload = {
        "hostname": hostname,
        "ssl": {"method": "http", "type": "dv", "settings": {"min_tls_version": "1.2"}},
    }
    response = _make_request(
        "POST",
        endpoint,
        token,
        "Cloudflare Create Custom Hostname API failed",
        json=payload,
        timeout=15,
    )
    if response and response.status_code == 200:
        return True, response.json().get("result", {})
    return False, "API Error"


def get_custom_hostname(hostname_id, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_04_sync_tunnels]
    if not token or not zone_id or not hostname_id:
        return False, "Missing credentials or hostname ID"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/custom_hostnames/{hostname_id}"
    response = _make_request(
        "GET", endpoint, token, "Cloudflare Get Custom Hostname API failed", timeout=15
    )
    if response and response.status_code == 200:
        return True, response.json().get("result", {})
    return False, "API Error"


def delete_custom_hostname(hostname_id, token, zone_id):
    # # Verified by [@ANCHOR: COMM_test_05_delete_tunnel]
    if not token or not zone_id or not hostname_id:
        return False, "Missing credentials or hostname ID"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/custom_hostnames/{hostname_id}"
    response = _make_request(
        "DELETE",
        endpoint,
        token,
        "Cloudflare Delete Custom Hostname API failed",
        timeout=15,
    )
    if response and response.status_code == 200:
        return True, "Custom hostname deleted successfully."
    # 404 means the hostname is not on the zone -- someone removed it in the
    # Cloudflare dashboard, or an earlier attempt succeeded and its response
    # was lost. Either way the caller's goal ("this hostname must not exist")
    # is already met, so reporting it as a failure would make
    # cloudflare.hostname.pending.delete retry forever against something that
    # is not there. _make_request deliberately returns the 404 response rather
    # than raising, which is what makes this distinguishable at all.
    if response is not None and response.status_code == 404:
        return True, "Custom hostname was already gone."
    return False, "API Error"



_CF_ID = re.compile(r"[0-9a-f]{32}")


def is_cloudflare_id(value):
    """Cloudflare's object ids are 32 lower-case hex characters. Ids are put into URL paths, so one that
    is not that shape (a value an administrator pasted in) is refused instead of interpolated."""
    return bool(value) and bool(_CF_ID.fullmatch(value))


def find_zone_id(name, token):
    """The id of the zone called exactly `name` that the token can see. (True, id), (True, None) when
    the token sees no such zone, or (False, message) when Cloudflare could not be asked. GET only."""
    # # Verified by [@ANCHOR: COMM_test_dns_api_calls]
    if not token or not name:
        return False, "Missing credentials or zone name"
    endpoint = "https://api.cloudflare.com/client/v4/zones"
    response = _make_request(
        "GET", endpoint, token, "Cloudflare Find Zone API failed", params={"name": name}, timeout=15
    )
    if response is None or response.status_code != 200:
        return False, "API Error"
    for zone in response.json().get("result") or []:
        if (zone.get("name") or "").lower() == name.lower():
            return True, zone.get("id")
    return True, None


def list_dns_records_named(zone_id, name, token):
    """Every DNS record of the zone under exactly `name` (all types): (True, [record, ...]) or
    (False, message). A failed read is never an empty list, so a caller can not mistake an outage for
    "nothing exists yet". GET only."""
    # # Verified by [@ANCHOR: COMM_test_dns_api_calls]
    if not token or not is_cloudflare_id(zone_id) or not name:
        return False, "Missing credentials, zone or name"
    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records"
    found = []
    page = 1
    while True:
        response = _make_request(
            "GET", endpoint, token, "Cloudflare List DNS Records API failed",
            params={"name": name, "per_page": 100, "page": page}, timeout=15,
        )
        if response is None or response.status_code != 200:
            return False, "API Error"
        body = response.json()
        found.extend(body.get("result") or [])
        if page >= int((body.get("result_info") or {}).get("total_pages") or 1):
            return True, found
        page += 1


def create_dns_record(zone_id, payload, token):
    # # Verified by [@ANCHOR: COMM_test_dns_api_calls]
    if not token or not is_cloudflare_id(zone_id):
        return False, "Missing credentials or zone"
    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records"
    response = _make_request(
        "POST", endpoint, token, "Cloudflare Create DNS Record API failed", json=payload, timeout=15
    )
    if response is not None and response.status_code == 200:
        return True, (response.json().get("result") or {}).get("id")
    return False, "API Error"


def update_dns_record(zone_id, record_id, payload, token):
    # # Verified by [@ANCHOR: COMM_test_dns_api_calls]
    if not token or not is_cloudflare_id(zone_id) or not is_cloudflare_id(record_id):
        return False, "Missing credentials, zone or record id"
    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records/{record_id}"
    response = _make_request(
        "PUT", endpoint, token, "Cloudflare Update DNS Record API failed", json=payload, timeout=15
    )
    if response is not None and response.status_code == 200:
        return True, record_id
    return False, "API Error"


def delete_dns_record(zone_id, record_id, token):
    """Delete one DNS record by id. Only the push's "retire" path calls this. 404 means it is already gone,
    which is what the caller wanted."""
    # # Verified by [@ANCHOR: COMM_test_dns_api_calls]
    if not token or not is_cloudflare_id(zone_id) or not is_cloudflare_id(record_id):
        return False, "Missing credentials, zone or record id"
    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records/{record_id}"
    response = _make_request(
        "DELETE", endpoint, token, "Cloudflare Delete DNS Record API failed", timeout=15
    )
    if response is not None and response.status_code in (200, 404):
        return True, record_id
    return False, "API Error"
