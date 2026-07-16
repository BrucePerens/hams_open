# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models


class ResConfigSettings(models.TransientModel):
    _inherit = 'res.config.settings'

    # [@ANCHOR: COMM_hams_s3_config]
    # # Verified by [@ANCHOR: COMM_COMM_hams_s3_config]
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

    @api.depends('hams_s3_use_s3')
    def _compute_hams_s3_oca_installed(self):
        for rec in self:
            rec.hams_s3_oca_installed = 'storage.backend' in self.env

    @api.model
    def get_values(self):
        res = super(ResConfigSettings, self).get_values()
        if 'storage.backend' in self.env:
            backend = self.env['storage.backend'].sudo().search(
                [('backend_type', '=', 'amazon_s3')], limit=1
            )
            if backend:
                res.update({
                    'hams_s3_aws_host': backend.aws_host,
                    'hams_s3_aws_access_key_id': backend.aws_access_key_id,
                    'hams_s3_aws_secret_access_key': backend.aws_secret_access_key,
                    'hams_s3_aws_bucket': backend.aws_bucket,
                    'hams_s3_aws_region': backend.aws_region,
                })
        return res

    def set_values(self):
        super(ResConfigSettings, self).set_values()
        if self.hams_s3_use_s3 and 'storage.backend' in self.env:
            backend = self.env['storage.backend'].sudo().search(
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
            if backend:
                backend.write(vals)
            else:
                self.env['storage.backend'].sudo().create(vals)
