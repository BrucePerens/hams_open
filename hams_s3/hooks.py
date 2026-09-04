# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# DRAFT, UNVERIFIED: written to register the service account this module's
# res_config_settings.py now uses in place of .sudo(). Mirrors
# distributed_redis_cache/hooks.py's post_init_hook exactly (same
# daemon.key.registry.register_daemon() call shape). Not exercised end to
# end -- see security/hams_s3_security.xml's own header comment for why.
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    # [@ANCHOR: hams_s3_post_init_hook]
    env["daemon.key.registry"].register_daemon(
        daemon_name="S3 Storage Manager",
        user_xml_id="hams_s3.s3_manager_service_internal",
        env_file_path="/opt/hams/etc/keys/s3_manager.env",
    )
    _logger.info("Registered S3 Storage Manager daemon keys.")
