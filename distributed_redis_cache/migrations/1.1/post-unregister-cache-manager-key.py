# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""distributed_redis_cache 1.1: stop minting the cache manager's unused Odoo API key.

The module used to register the "Redis Cache Manager" daemon with the Daemon Key Manager at install.
The cache-manager daemon never used the key, so 1.1 no longer registers it. ``post_init_hook`` is gone
too, so an installed database drops the registry row and revokes the key here. Idempotent."""

from odoo import api

from odoo.addons.distributed_redis_cache.hooks import (
    KEY_MANAGER_XMLID,
    unregister_cache_manager_key,
)


def migrate(cr, version):
    if not version:
        return
    module, name = KEY_MANAGER_XMLID.split(".", 1)
    cr.execute(
        "SELECT res_id FROM ir_model_data WHERE module = %s AND name = %s AND model = 'res.users'",
        (module, name),
    )
    row = cr.fetchone()
    if not row:
        raise ValueError(f"{KEY_MANAGER_XMLID} is missing")
    unregister_cache_manager_key(api.Environment(cr, row[0], {}))
