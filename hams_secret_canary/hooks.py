# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later


# [@ANCHOR: hams_secret_canary:post_init_hook]
def post_init_hook(env):
    """Creates this install's salt and plants the decoy credentials (`hams.secret.canary._plant_canaries`). Idempotent:
    an install that already has a salt and decoys keeps them."""
    env["hams.secret.canary"]._plant_canaries()
