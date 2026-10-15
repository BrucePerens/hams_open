# SPDX-License-Identifier: AGPL-3.0-or-later
import secrets


# [@ANCHOR: form_shield:post_init_hook]
# Verified by [@ANCHOR: form_shield:test_post_init_hook_seeds_key_once]
def post_init_hook(env):
    """Seeds the signing key and the documentation article. An existing key is never replaced:
    replacing it would invalidate every challenge in flight, harmlessly, but needlessly."""
    params = env["ir.config_parameter"]
    if not params.get_param("form_shield.signing_key"):
        params.set_param("form_shield.signing_key", secrets.token_hex(32))
    utils = env["zero_sudo.security.utils"]
    svc_uid = utils._get_service_uid("cloudflare.user_cloudflare_waf")
    module = env["ir.module.module"].with_user(svc_uid)
    module._bootstrap_knowledge_docs()
