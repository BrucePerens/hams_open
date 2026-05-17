# -*- coding: utf-8 -*-
import requests
import logging
import base64
import os

_logger = logging.getLogger(__name__)


def purge_urls(urls, token, zone_id):
    if not token or not zone_id:
        return False
    if not urls:
        return True

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    # Batch requests into chunks of 30 items
    success = True
    for i in range(0, len(urls), 30):
        chunk = urls[i : i + 30]
        payload = {"files": chunk}
        try:
            response = requests.post(
                endpoint, json=payload, headers=headers, timeout=10
            )
            response.raise_for_status()
        except requests.exceptions.RequestException as e:
            _logger.error(f"Cloudflare URL purge API failed for chunk: {e}")
            success = False
    return success


def purge_tags(tags, token, zone_id):
    if not token or not zone_id:
        return False
    if not tags:
        return True

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    # Batch requests into chunks of 30 items
    success = True
    for i in range(0, len(tags), 30):
        chunk = tags[i : i + 30]
        payload = {"tags": chunk}
        try:
            response = requests.post(
                endpoint, json=payload, headers=headers, timeout=10
            )
            response.raise_for_status()
        except requests.exceptions.RequestException as e:
            _logger.error(f"Cloudflare Tag purge API failed for chunk: {e}")
            success = False
    return success


def ban_ip(ip_address, mode, notes, token, zone_id):
    if not token or not zone_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/firewall/access_rules/rules"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    payload = {
        "mode": mode,
        "configuration": {"target": "ip", "value": ip_address},
        "notes": notes,
    }

    try:
        response = requests.post(endpoint, json=payload, headers=headers, timeout=10)
        response.raise_for_status()
        rule_id = response.json().get("result", {}).get("id")
        return True, rule_id
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare WAF IP Ban API failed: {e}")
        return False, str(e)


def unban_ip(rule_id, token, zone_id):
    if not token or not zone_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/firewall/access_rules/rules/{rule_id}"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.delete(endpoint, headers=headers, timeout=10)
        response.raise_for_status()
        return True, "Success"
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare WAF IP Unban API failed: {e}")
        return False, str(e)


def verify_turnstile(token, remote_ip, secret):
    if not secret or not token:
        return False

    endpoint = "https://challenges.cloudflare.com/turnstile/v0/siteverify"
    data = {"secret": secret, "response": token}
    if remote_ip:
        data["remoteip"] = remote_ip

    try:
        response = requests.post(endpoint, data=data, timeout=10)
        return response.json().get("success", False)
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Turnstile verification failed: {e}")
        return False


def get_zone_ruleset(phase, token, zone_id):
    if not token or not zone_id:
        return None

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/rulesets/phases/{phase}/entrypoint"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.get(endpoint, headers=headers, timeout=15)
        if response.status_code == 404:
            return None
        response.raise_for_status()
        return response.json().get("result")
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Ruleset Fetch API failed: {e}")
        return None


def update_zone_ruleset(ruleset_id, payload, token, zone_id):
    if not token or not zone_id:
        return False, "Missing credentials."

    endpoint = (
        f"https://api.cloudflare.com/client/v4/zones/{zone_id}/rulesets/{ruleset_id}"
    )
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.put(endpoint, json=payload, headers=headers, timeout=15)
        response.raise_for_status()
        return True, "Ruleset updated successfully."
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Ruleset Update API failed: {e}")
        return False, str(e)


def create_zone_ruleset(payload, token, zone_id):
    if not token or not zone_id:
        return False, "Missing credentials."

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/rulesets"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.post(endpoint, json=payload, headers=headers, timeout=15)
        response.raise_for_status()
        return True, "Ruleset created successfully."
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Ruleset Create API failed: {e}")
        return False, str(e)


def create_cfd_tunnel(account_id, token, tunnel_name):
    if not token or not account_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    # Cloudflare requires a >= 32 byte base64 encoded secret for tunnel creation
    secret = base64.b64encode(os.urandom(32)).decode("utf-8")
    payload = {"name": tunnel_name, "tunnel_secret": secret}

    try:
        response = requests.post(endpoint, json=payload, headers=headers, timeout=15)
        response.raise_for_status()
        return True, response.json().get("result", {}).get("id")
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Tunnel Create API failed: {e}")
        return False, str(e)


def get_cfd_tunnel_token(account_id, token, tunnel_id):
    if not token or not account_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/token"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.get(endpoint, headers=headers, timeout=15)
        response.raise_for_status()
        return True, response.json().get("result", "")
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Tunnel Token API failed: {e}")
        return False, str(e)

def purge_everything(token, zone_id):
    if not token or not zone_id:
        return False

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
    payload = {"purge_everything": True}

    try:
        response = requests.post(endpoint, json=payload, headers=headers, timeout=10)
        response.raise_for_status()
        return True
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Purge Everything API failed: {e}")
        return False


def get_zone_settings(token, zone_id):
    if not token or not zone_id:
        return None

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/settings"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.get(endpoint, headers=headers, timeout=10)
        if response.status_code == 404:
            return None
        response.raise_for_status()
        return response.json().get("result")
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Get Zone Settings API failed: {e}")
        return None


def update_zone_setting(setting_name, value, token, zone_id):
    if not token or not zone_id:
        return False, "Missing credentials"

    endpoint = f"https://api.cloudflare.com/client/v4/zones/{zone_id}/settings/{setting_name}"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
    payload = {"value": value}

    try:
        response = requests.patch(endpoint, json=payload, headers=headers, timeout=10)
        response.raise_for_status()
        return True, "Setting updated successfully."
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Update Zone Setting API failed: {e}")
        return False, str(e)


def list_cfd_tunnels(account_id, token):
    if not token or not account_id:
        return []

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.get(endpoint, headers=headers, timeout=15)
        response.raise_for_status()
        return response.json().get("result", [])
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare List Tunnels API failed: {e}")
        return []


def delete_cfd_tunnel(account_id, token, tunnel_id):
    if not token or not account_id or not tunnel_id:
        return False, "Missing credentials or tunnel ID"

    endpoint = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}

    try:
        response = requests.delete(endpoint, headers=headers, timeout=15)
        response.raise_for_status()
        return True, "Tunnel deleted successfully."
    except requests.exceptions.RequestException as e:
        _logger.error(f"Cloudflare Delete Tunnel API failed: {e}")
        return False, str(e)
