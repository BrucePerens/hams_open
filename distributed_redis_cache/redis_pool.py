# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import os
import logging
import redis
import threading

_logger = logging.getLogger(__name__)

POOL_LOCK = threading.Lock()

# [@ANCHOR: COMM_redis_connection_pool]
# Default values used as fallback when the Odoo registry is not yet available
# or during initial module loading.
REDIS_HOST_DEFAULT = os.getenv("REDIS_HOST") or "redis"
REDIS_PORT_DEFAULT = int(os.getenv("REDIS_PORT") or "6379")
REDIS_PASS_DEFAULT = os.getenv("REDIS_PASSWORD")  # burn-ignore-env: # Tested by [@ANCHOR: COMM_test_redis_pool_env_variables]
REDIS_DB_DEFAULT = int(os.getenv("REDIS_DB") or "0")

# Centralized connection pool for the default Redis settings
redis_pool = redis.ConnectionPool(
    host=REDIS_HOST_DEFAULT,
    port=REDIS_PORT_DEFAULT,
    password=REDIS_PASS_DEFAULT,
    db=REDIS_DB_DEFAULT,
    decode_responses=True,
    socket_timeout=1.0,
    socket_connect_timeout=1.0,
)

# Registry to cache connection pools for custom configurations to avoid connection churn
_custom_pools = {}


# Registry to cache DB configs to avoid repeated queries
_db_configs = {}

def get_redis_connection(env=None):
    """
    Returns a Redis client using settings from the environment if available,
    otherwise falling back to the centralized connection pool.
    """
    if env:
        dbname = env.cr.dbname
        with POOL_LOCK:
            if dbname in _db_configs:
                host, port, password = _db_configs[dbname]
            else:
                # Configuration is loaded via zero_sudo security utils to comply with security mandates
                security_utils = env["zero_sudo.security.utils"].with_context(redis_bypass_cache=True)
                host = security_utils._get_system_param(
                    "distributed_redis_cache.redis_host", REDIS_HOST_DEFAULT
                )
                port_raw = security_utils._get_system_param(
                    "distributed_redis_cache.redis_port", str(REDIS_PORT_DEFAULT)
                )
                try:
                    port = int(port_raw)
                except (ValueError, TypeError):
                    port = REDIS_PORT_DEFAULT
                password = security_utils._get_system_param(
                    "distributed_redis_cache.redis_password", REDIS_PASS_DEFAULT
                )
                _db_configs[dbname] = (host, port, password)

        # Check if the environment config differs from the default pool
        if (
            host != REDIS_HOST_DEFAULT
            or port != REDIS_PORT_DEFAULT
            or password != REDIS_PASS_DEFAULT
        ):
            pool_key = (host, port, password)
            with POOL_LOCK:
                if pool_key not in _custom_pools:
                    _custom_pools[pool_key] = redis.ConnectionPool(
                        host=host,
                        port=port,
                        password=password,
                        db=REDIS_DB_DEFAULT,
                        decode_responses=True,
                        socket_timeout=1.0,
                        socket_connect_timeout=1.0,
                    )
            return redis.Redis(connection_pool=_custom_pools[pool_key])

    _logger.debug("Returning default Redis Connection Pool.")
    return redis.Redis(connection_pool=redis_pool)
