# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from . import models

def _post_init_hook(env):
    """Inject documentation."""
    env["ir.module.module"]._bootstrap_knowledge_docs()
