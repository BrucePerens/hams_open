# SPDX-License-Identifier: AGPL-3.0-or-later


# [@ANCHOR: tenant_sites:COMM_post_init_hook]
# Verified by [@ANCHOR: tenant_sites:COMM_test_post_init_hook]
def post_init_hook(env):
    """Installs the end-user documentation. The module changes no behaviour until a tenant site
    or parked domain exists, so nothing else is seeded."""
    utils = env["zero_sudo.security.utils"]
    svc_uid = utils._get_service_uid("cloudflare.user_cloudflare_waf")
    env["ir.module.module"].with_user(svc_uid)._bootstrap_knowledge_docs()
