# SPDX-License-Identifier: AGPL-3.0-or-later
import secrets


def post_init_hook(env):
    """Seeds the settings the public handler needs. Existing values are never overwritten."""
    params = env["ir.config_parameter"]
    if not params.get_param("parking.form_secret"):
        params.set_param("parking.form_secret", secrets.token_hex(32))
    if not params.get_param("parking.unknown_host_policy"):
        params.set_param("parking.unknown_host_policy", "not_found")
