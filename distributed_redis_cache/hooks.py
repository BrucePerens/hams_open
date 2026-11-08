# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

_logger = logging.getLogger(__name__)

KEY_MANAGER_XMLID = "daemon_key_manager.user_daemon_key_manager_service"
CACHE_MANAGER_KEY_NAME = "Redis Cache Manager"


# [@ANCHOR: distributed_redis_cache:unregister_cache_manager_key]
# Verified by [@ANCHOR: distributed_redis_cache:test_unregister_cache_manager_key]
def unregister_cache_manager_key(env):
    """Removes the Daemon Key Manager registry row "Redis Cache Manager" and revokes the API key it
    minted for the cache manager service account, through ``daemon.key.registry.unregister_daemon``.
    Returns the number of registry rows removed.

    The cache-manager daemon never used that key (it has no Odoo client; its only credential is the
    CONNECT-only PostgreSQL login in ``keys/cache_manager/cache_manager_db.env``), but the registry
    kept rotating it into ``keys/cache_manager.env``. Version 1.1 stops registering it
    (``migrations/1.1`` calls this once for an installed database).

    Runs as the Daemon Key Manager service account, the account that owns the registry. The service
    account ``distributed_redis_cache.cache_manager_service_internal`` stays, with no key. The key
    file is left on disk: the registry never deletes one, and an operator removes it.
    Idempotent: a database with no such row removes nothing and returns 0. Raises ``AccessError`` when
    the Daemon Key Manager service account is missing or unusable (the upgrade then stops, which is
    intended: a half-removed registration must not be left behind). ``env`` may be any environment;
    nothing here runs as the superuser, and nothing commits.
    """
    manager_uid = env["zero_sudo.security.utils"]._get_service_uid(KEY_MANAGER_XMLID)
    registry = env["daemon.key.registry"].with_user(manager_uid)
    count = registry.unregister_daemon(CACHE_MANAGER_KEY_NAME)
    if count:
        _logger.info("Removed %d unused Redis Cache Manager key registration(s).", count)
    return count
