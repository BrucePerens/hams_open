# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Per-network request counting for anonymous endpoints (the same Redis counter /demo/enter uses)."""
import ipaddress
import logging

import redis
from odoo.addons.distributed_redis_cache.redis_pool import expiring_counter, redis_pool

_logger = logging.getLogger(__name__)


# [@ANCHOR: knowledge:network_of]
# Verified by [@ANCHOR: test_manual_feedback_is_limited_per_network]
def network_of(address):
    """The /24 (IPv4) or /48 (IPv6) network of `address` as text; "none" when it is missing or unparseable."""
    try:
        parsed = ipaddress.ip_address((address or "").strip())
    except ValueError:
        return "none"
    if parsed.version == 6 and parsed.ipv4_mapped is not None:
        parsed = parsed.ipv4_mapped
    return str(ipaddress.ip_network(f"{parsed}/{24 if parsed.version == 4 else 48}", strict=False))


# [@ANCHOR: knowledge:network_hits]
# Verified by [@ANCHOR: test_manual_feedback_is_limited_per_network]
def network_hits(env, name, window_seconds):
    """Counts one hit of `name` against the caller's network and returns the count in this window, or None when
    Redis is down. The caller decides what an outage means: refuse (a write or a costly read) or let the request
    through (a service that must keep answering)."""
    address = env["zero_sudo.security.utils"]._get_trusted_client_ip()
    key = "%s:ratelimit:%s:%s" % (env.cr.dbname, name, network_of(address))
    try:
        return expiring_counter(redis.Redis(connection_pool=redis_pool), key, window_seconds)
    except redis.RedisError as error:
        _logger.warning("Redis error during the %s limit: %s", name, error)
        return None
