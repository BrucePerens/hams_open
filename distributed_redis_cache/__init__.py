# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later

from . import models
from . import redis_cache
from . import redis_pool
from . import hooks
# No post_init_hook since 1.1: the unused Daemon Key Manager registration is removed by
# hooks.unregister_cache_manager_key(), which migrations/1.1 calls.
