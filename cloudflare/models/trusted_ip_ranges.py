# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""
[@ANCHOR: cloudflare:trusted_ip_ranges]
Verified by [@ANCHOR: test_trusted_ip_ranges]

Cloudflare Tunnel deployments (this project's own real deployment on hams.com) never need this:
`cloudflared` forwards to the origin over plain loopback HTTP, so the origin's REMOTE_ADDR is
always 127.0.0.1/::1, never one of Cloudflare's published edge IPs -- the loopback check in
`wsgi_proxy_scheme.py`/`edge_context.py` already covers that case on its own, unconditionally.

This module exists for the OTHER real self-hosting topology: "orange-cloud" Cloudflare proxying
with no Tunnel, where Cloudflare's edge connects to the origin directly over the network and
REMOTE_ADDR genuinely is one of Cloudflare's published ranges. Without this, a self-hosted admin
in that topology had no way to let Odoo trust CF-* headers at all -- the existing loopback-only
check can never match a real network peer.

Cloudflare's published ranges change occasionally, so the effective list is a MERGE of two parts,
both admin-visible on the Settings page:
- an auto-fetched list, periodically refreshed from https://www.cloudflare.com/ips-v4 and
  /ips-v6 by `_cron_refresh_cloudflare_ip_ranges` below, seeded from the `_DEFAULT_CLOUDFLARE_*`
  snapshot constants (fetched live 2026-09-26) so the feature works correctly before the cron's
  first run and stays safe if a later fetch ever fails (a failed/empty/malformed fetch never
  overwrites the last good value);
- an admin-supplied custom list (`cloudflare.trusted_ip_ranges_custom`), for ranges the admin
  wants to add on top (e.g. a corporate VPN's egress or a second CDN) -- never touched by the
  cron, so a manual edit is never silently clobbered by the next scheduled refresh.

The merged, parsed result is published to Redis (`get_redis_connection()`, no `env` needed) so
`wsgi_proxy_scheme.py`'s WSGI-entry-point hook -- which runs before any Odoo `env`/cursor exists
for the request, see that module's own docstring for why it has to run that early -- can check an
incoming peer against it without opening a database connection on every single HTTP request.
"""
import ipaddress
import json
import logging

import requests

from odoo import models, fields, api
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection

_logger = logging.getLogger(__name__)

# Fetched live from https://www.cloudflare.com/ips-v4 and /ips-v6, 2026-09-26. Used to seed the
# auto-fetched list before the cron's first run, and as the fallback if a later fetch ever comes
# back empty, unparseable, or fails outright -- see _cron_refresh_cloudflare_ip_ranges() below.
DEFAULT_CLOUDFLARE_IPV4_RANGES = (
    "173.245.48.0/20",
    "103.21.244.0/22",
    "103.22.200.0/22",
    "103.31.4.0/22",
    "141.101.64.0/18",
    "108.162.192.0/18",
    "190.93.240.0/20",
    "188.114.96.0/20",
    "197.234.240.0/22",
    "198.41.128.0/17",
    "162.158.0.0/15",
    "104.16.0.0/13",
    "104.24.0.0/14",
    "172.64.0.0/13",
    "131.0.72.0/22",
)
DEFAULT_CLOUDFLARE_IPV6_RANGES = (
    "2400:cb00::/32",
    "2606:4700::/32",
    "2803:f800::/32",
    "2405:b500::/32",
    "2405:8100::/32",
    "2a06:98c0::/29",
    "2c0f:f248::/32",
)

CONFIG_KEY_AUTO = "cloudflare.trusted_ip_ranges_auto"
CONFIG_KEY_CUSTOM = "cloudflare.trusted_ip_ranges_custom"
CONFIG_KEY_LAST_REFRESHED = "cloudflare.trusted_ip_ranges_last_refreshed"

REDIS_KEY = "cloudflare:trusted_ip_ranges"
# How long a self-hosted admin's manual Redis outage is tolerated before the WSGI hook falls back
# to the loopback-only check plus this module's own baked-in default -- see
# wsgi_proxy_scheme.py's _is_trusted_cf_peer() for the consumer side of this constant's twin.
REDIS_KEY_TTL_SECONDS = 3600


def _parse_ranges(text):
    """Splits `text` on newlines/commas/whitespace and returns the subset of tokens that parse
    as a real IPv4/IPv6 network. Malformed tokens (typos, blank lines) are skipped and logged,
    never allowed to abort the whole refresh -- a single bad line in an admin's custom list must
    not take down the entire trusted-range check."""
    ranges = []
    for raw in (text or "").replace(",", "\n").splitlines():
        token = raw.strip()
        if not token:
            continue
        try:
            ipaddress.ip_network(token, strict=False)
        except ValueError:
            _logger.warning("Skipping malformed Cloudflare trusted IP range: %r", token)
            continue
        ranges.append(token)
    return ranges


class CloudflareTrustedIpUtils(models.AbstractModel):
    _name = "cloudflare.trusted_ip_utils"
    _description = "Cloudflare Trusted IP Range Utilities"

    @api.model
    # [@ANCHOR: cloudflare:get_effective_trusted_ip_ranges]
    def _get_effective_trusted_ip_ranges(self):
        """The merged auto + custom range list, as validated CIDR strings. The auto half falls
        back to this module's own baked-in default snapshot when empty (first install, before
        the cron's first successful run)."""
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_trusted_ip"
        )
        icp = self.env["ir.config_parameter"].with_user(svc_uid)
        auto_text = icp.get_param(CONFIG_KEY_AUTO, "")
        auto_ranges = _parse_ranges(auto_text) or list(
            DEFAULT_CLOUDFLARE_IPV4_RANGES + DEFAULT_CLOUDFLARE_IPV6_RANGES
        )
        custom_ranges = _parse_ranges(icp.get_param(CONFIG_KEY_CUSTOM, ""))
        # dict.fromkeys(): de-duplicate while preserving order, in case the admin's custom list
        # happens to repeat something already in the auto-fetched list.
        return list(dict.fromkeys(auto_ranges + custom_ranges))

    @api.model
    # [@ANCHOR: cloudflare:is_trusted_cf_peer_with_env]
    def _is_trusted_cf_peer(self, remote_addr):
        """True when `remote_addr` is either loopback (this deployment's own Cloudflare Tunnel
        case) or falls inside the effective trusted-range allow-list (the non-Tunnel case). Used
        by callers that already have an `env` (e.g. edge_context.py); the env-less WSGI entry
        hook (wsgi_proxy_scheme.py) has its own equivalent that reads the same data from Redis
        instead, since it runs before any `env` exists for the request."""
        if remote_addr in ("127.0.0.1", "::1"):  # burn-ignore-tunnel-peer-check
            return True
        try:
            peer = ipaddress.ip_address(remote_addr)
        except ValueError:
            return False
        for cidr in self._get_effective_trusted_ip_ranges():
            if peer in ipaddress.ip_network(cidr):
                return True
        return False

    @api.model
    # [@ANCHOR: cloudflare:publish_trusted_ip_ranges_to_redis]
    def _publish_trusted_ip_ranges_to_redis(self):
        """Pushes the current merged range list to Redis so the env-less WSGI hook
        (wsgi_proxy_scheme.py) can read it without a database connection. Called after every
        cron refresh and every admin save of the custom-ranges settings field, so a change takes
        effect immediately rather than waiting for the next cron tick."""
        ranges = self._get_effective_trusted_ip_ranges()
        try:
            r = get_redis_connection(self.env)
            r.set(REDIS_KEY, json.dumps(ranges), ex=REDIS_KEY_TTL_SECONDS)
        except Exception as e:  # audit-ignore-catch-all
            # Redis being unreachable must never break a settings save or a cron tick -- the
            # WSGI hook's own fallback (loopback check + baked-in default) covers this case.
            _logger.warning("Could not publish Cloudflare trusted IP ranges to Redis: %s", e)

    @api.model
    # [@ANCHOR: cloudflare:cron_refresh_cloudflare_ip_ranges]
    # Verified by [@ANCHOR: test_cron_refresh_cloudflare_ip_ranges]
    def _cron_refresh_cloudflare_ip_ranges(self):
        """Refreshes the auto-fetched half of the trusted range list from Cloudflare's own
        published lists. Never overwrites the existing value with an empty or malformed result --
        a fetch failure (network error, Cloudflare serving something unexpected) leaves the last
        known-good list in place rather than silently disabling the allow-list."""
        fetched = []
        # audit-ignore-outbound-fetch: both URLs are fixed literals in this tuple, never derived
        # from a caller, a record field, or an HTTP parameter -- there is no SSRF surface here.
        for url in ("https://www.cloudflare.com/ips-v4", "https://www.cloudflare.com/ips-v6"):
            try:
                response = requests.get(url, timeout=10)  # audit-ignore-outbound-fetch
                response.raise_for_status()
            except requests.exceptions.RequestException as e:
                _logger.warning("Could not fetch Cloudflare IP ranges from %s: %s", url, e)
                continue
            fetched.extend(_parse_ranges(response.text))

        if not fetched:
            _logger.warning(
                "Cloudflare IP range refresh produced no valid ranges; keeping the existing list."
            )
            return

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_trusted_ip"
        )
        icp = self.env["ir.config_parameter"].with_user(svc_uid)
        icp.set_param(CONFIG_KEY_AUTO, "\n".join(fetched))
        icp.set_param(CONFIG_KEY_LAST_REFRESHED, fields.Datetime.to_string(fields.Datetime.now()))
        self._publish_trusted_ip_ranges_to_redis()
        _logger.info("Refreshed Cloudflare trusted IP ranges: %d entries.", len(fetched))
