# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
from . import patch, unix_listener


# [@ANCHOR: hams_rpc_guard:post_init_hook]
def post_init_hook(env):
    """Installs the module's end-user documentation article. Sets no system parameter: the defaults in code
    are the safe ones, and an operator's explicit choice is never overwritten."""
    env["ir.module.module"]._bootstrap_knowledge_docs()


# [@ANCHOR: hams_rpc_guard:post_load_hook]
def post_load_hook():
    """Installs the method guard (`patch.install`) and the Unix-socket server hooks (`unix_listener.install_server_patches`,
    which open no socket unless the server configuration names one) when the module is loaded as a server-wide module,
    before any request is served and, under `--workers`, in the master before any worker is forked.

    Takes no arguments, returns nothing, and is idempotent: loading the module twice wraps nothing twice."""
    patch.install()
    unix_listener.install_server_patches()
