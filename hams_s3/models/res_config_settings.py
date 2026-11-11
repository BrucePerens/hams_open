# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

from odoo import _, api, fields, models
from odoo.exceptions import UserError

from . import b2_key_scope

_logger = logging.getLogger(__name__)

# What the Settings form shows in place of a stored S3 secret key. Saving the form with this value (or an empty one)
# leaves the stored key as it is; any other value replaces it.
SECRET_MASK = "********"


class ResConfigSettings(models.TransientModel):
    _inherit = 'res.config.settings'

    # [@ANCHOR: COMM_hams_s3_config]
    hams_s3_use_s3 = fields.Boolean(
        string="Use Amazon S3 Storage",
        config_parameter="hams_s3.use_s3_storage",
        help="Enable and configure the Amazon S3 storage backend."
    )

    hams_s3_oca_installed = fields.Boolean(
        string="OCA Storage Installed",
        compute="_compute_hams_s3_oca_installed"
    )

    hams_s3_aws_host = fields.Char(string="AWS Host")
    hams_s3_aws_access_key_id = fields.Char(string="AWS Access Key")
    hams_s3_aws_secret_access_key = fields.Char(string="AWS Secret Key")
    hams_s3_aws_bucket = fields.Char(string="AWS Bucket")
    hams_s3_aws_region = fields.Selection([
        ('us-east-1', 'US East (N. Virginia)'),
        ('us-east-2', 'US East (Ohio)'),
        ('us-west-1', 'US West (N. California)'),
        ('us-west-2', 'US West (Oregon)'),
        ('eu-west-1', 'EU (Ireland)'),
        ('eu-central-1', 'EU (Frankfurt)'),
        ('ap-southeast-1', 'Asia Pacific (Singapore)'),
        ('ap-southeast-2', 'Asia Pacific (Sydney)'),
        ('ap-northeast-1', 'Asia Pacific (Tokyo)'),
        ('sa-east-1', 'South America (Sao Paulo)'),
        ('other', 'Other'),
    ], string="AWS Region")

    # The 'model' in self.env presence checks below are a deliberate,
    # reviewed exception (# burn-ignore-optional-oca-dep, see
    # check_burn_list.py's whitelist entry for the full rationale):
    # storage.backend comes from OCA's storage_backend addon, which
    # hams_s3 deliberately does NOT hard-depend on -- see
    # scripts/install_oca_storage.py, whose whole purpose is to let an
    # admin install that addon *after* hams_s3 is already running.
    # Declaring it a hard __manifest__.py dependency would break that
    # bootstrap flow entirely.
    #
    # DRAFT, UNVERIFIED: the .sudo() calls that used to be here were
    # replaced with the service account registered in
    # security/hams_s3_security.xml / hooks.py (same pattern
    # distributed_redis_cache uses). This has NOT been exercised against a
    # real storage_backend install -- that addon isn't present anywhere in
    # this sandbox, so whether s3_manager_service_internal's group actually
    # has the read/write/create access storage_backend's own security rules
    # require on storage.backend is unverified. Review and test against a
    # real storage_backend install before relying on this.
    def _get_s3_service_env(self):
        # [@ANCHOR: hams_s3_get_s3_service_env]
        return self.env["zero_sudo.security.utils"]._get_service_env(
            "hams_s3.s3_manager_service_internal"
        )

    @api.depends('hams_s3_use_s3')
    def _compute_hams_s3_oca_installed(self):
        # [@ANCHOR: hams_s3_compute_oca_installed]
        for rec in self:
            rec.hams_s3_oca_installed = 'storage.backend' in self.env  # burn-ignore-optional-oca-dep

    @api.private
    @api.model
    def get_values(self):
        # [@ANCHOR: hams_s3_get_values]
        res = super(ResConfigSettings, self).get_values()
        if 'storage.backend' in self.env:  # burn-ignore-optional-oca-dep
            env_svc = self._get_s3_service_env()
            backend = env_svc['storage.backend'].search(
                [('backend_type', '=', 'amazon_s3')], limit=1
            )
            if backend:
                res.update({
                    'hams_s3_aws_host': backend.aws_host,
                    'hams_s3_aws_access_key_id': backend.aws_access_key_id,
                    'hams_s3_aws_secret_access_key': SECRET_MASK if backend.aws_secret_access_key else False,
                    'hams_s3_aws_bucket': backend.aws_bucket,
                    'hams_s3_aws_region': backend.aws_region,
                })
        return res

    def set_values(self):
        # [@ANCHOR: hams_s3_set_values]
        self._check_b2_key_scope()
        super(ResConfigSettings, self).set_values()
        self._register_s3_secret_with_canary()
        if self.hams_s3_use_s3 and 'storage.backend' in self.env:  # burn-ignore-optional-oca-dep
            env_svc = self._get_s3_service_env()
            backend = env_svc['storage.backend'].search(
                [('backend_type', '=', 'amazon_s3')], limit=1
            )
            vals = {
                'name': 'Amazon S3 Storage',
                'backend_type': 'amazon_s3',
                'aws_host': self.hams_s3_aws_host,
                'aws_access_key_id': self.hams_s3_aws_access_key_id,
                'aws_secret_access_key': self.hams_s3_aws_secret_access_key,
                'aws_bucket': self.hams_s3_aws_bucket,
                'aws_region': self.hams_s3_aws_region or 'us-east-1',
            }
            if self.hams_s3_aws_secret_access_key in (SECRET_MASK, False, ''):
                if backend:
                    del vals['aws_secret_access_key']  # unchanged in the form: keep the stored key
                else:
                    vals['aws_secret_access_key'] = False
            if backend:
                backend.write(vals)
            else:
                env_svc['storage.backend'].create(vals)

    def _check_b2_key_scope(self):
        """When the S3 endpoint is Backblaze B2 and a new secret key is being saved, asks Backblaze what the key may do
        and refuses (unless overridden) a key that can delete files or buckets or manage keys, one that is not limited
        to a bucket, and one Backblaze cannot be asked about. The override is the system parameter
        `hams_s3.allow_broad_b2_key` set to `True`, which accepts the key with a logged warning."""
        if not self.hams_s3_use_s3 or not b2_key_scope.is_b2_host(self.hams_s3_aws_host):
            return
        secret = self.hams_s3_aws_secret_access_key
        if secret in (SECRET_MASK, False, ''):
            return
        ok, detail = b2_key_scope.inspect_b2_key(self.hams_s3_aws_access_key_id, secret)
        if ok and not detail:
            return
        problem = detail if not ok else _(
            "This Backblaze key is broader than a storage key needs: %s. Create an application key limited to one "
            "bucket without those capabilities.", ", ".join(detail))
        if self.env["ir.config_parameter"].get_param("hams_s3.allow_broad_b2_key") == "True":
            _logger.warning("Backblaze key accepted by override (hams_s3.allow_broad_b2_key): %s", problem)
            return
        raise UserError(problem + " " + _("(Set the system parameter hams_s3.allow_broad_b2_key to True to override.)"))

    def _register_s3_secret_with_canary(self):
        """When the optional `hams_secret_canary` addon is installed, registers the fingerprint of a newly entered S3
        secret key so that its appearance in a response or a log line raises an alarm. Only a salted hash is kept.
        An unchanged (masked) or empty value, or one that is not a single token, is skipped: saving never fails
        because of the canary."""
        secret = self.hams_s3_aws_secret_access_key
        if "hams.secret.canary" not in self.env or secret in (SECRET_MASK, False, ''):
            return
        try:
            self.env["hams.secret.canary"]._register_secret("hams_s3_aws_secret_access_key", secret)
        except ValueError as error:
            _logger.warning("Not registered with the secret canary: %s", error)
