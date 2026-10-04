# SPDX-License-Identifier: AGPL-3.0-or-later
import secrets


# [@ANCHOR: parking:COMM_post_init_hook]
# Verified by [@ANCHOR: parking:COMM_test_post_init_hook]
def post_init_hook(env):
    """Seeds the settings the public handler needs. Existing values are never overwritten."""
    params = env["ir.config_parameter"]
    if not params.get_param("parking.form_secret"):
        params.set_param("parking.form_secret", secrets.token_hex(32))
    if not params.get_param("parking.unknown_host_policy"):
        params.set_param("parking.unknown_host_policy", "not_found")
    utils = env["zero_sudo.security.utils"]
    svc_uid = utils._get_service_uid("cloudflare.user_cloudflare_waf")
    env["ir.module.module"].with_user(svc_uid)._bootstrap_knowledge_docs()
