# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import requests
import logging
import base64
import secrets
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry

_logger = logging.getLogger(__name__)


def _handle_api_error(context_msg, exception):
    if "External requests verboten" in str(exception):
        _logger.info("%s (Disabled in tests): %s", context_msg, exception)
    else:
        _logger.error("%s: %s", context_msg, exception)


session = requests.Session()
retry_strategy = Retry(
    total=3,
    status_forcelist=[429, 500, 502, 503, 504],
    allowed_methods=["HEAD", "GET", "OPTIONS", "POST", "DELETE", "PUT", "PATCH"],
)
adapter = HTTPAdapter(max_retries=retry_strategy)
session.mount("https://", adapter)
session.mount("http://", adapter)


def _make_request(method, endpoint, token, error_msg, **kwargs):
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
    if "headers" in kwargs:
        headers.update(kwargs.pop("headers"))

    timeout = kwargs.pop("timeout", 15)

    try:
        if method.upper() == "GET":
            response = session.get(endpoint, headers=headers, timeout=timeout, **kwargs)
        elif method.upper() == "POST":
            response = session.post(
                endpoint, headers=headers, timeout=timeout, **kwargs
            )
        elif method.upper() == "PUT":
            response = session.put(endpoint, headers=headers, timeout=timeout, **kwargs)
        elif method.upper() == "PATCH":
            response = session.patch(
                endpoint, headers=headers, timeout=timeout, **kwargs
            )
        elif method.upper() == "DELETE":
            response = session.delete(
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
        response = session.post(endpoint, data=data, timeout=10)
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
    return False, "API Error"
