# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later


# [@ANCHOR: hams_rpc_guard:post_init_hook]
def post_init_hook(env):
    """Installs the module's end-user documentation article. Sets no system parameter: the defaults in code
    are the safe ones, and an operator's explicit choice is never overwritten."""
    env["ir.module.module"]._bootstrap_knowledge_docs()
