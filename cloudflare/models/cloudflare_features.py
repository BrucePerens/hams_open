# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo import models, fields

class CloudflareDNSRecord(models.Model):
    _name = 'cloudflare.dns.record'
    _description = 'Cloudflare DNS Record'

    name = fields.Char(string='Name', required=True)
    type = fields.Selection([
        ('A', 'A'),
        ('AAAA', 'AAAA'),
        ('CNAME', 'CNAME'),
        ('TXT', 'TXT')
    ], string='Type', required=True)
    content = fields.Char(string='Content', required=True)
    proxied = fields.Boolean(string='Proxied', default=True)
    website_id = fields.Many2one('website', string='Website')

class CloudflareZoneSettings(models.Model):
    _name = 'cloudflare.zone.settings'
    _description = 'Cloudflare Zone Settings'

    name = fields.Char(string='Name', required=True)
    ssl_mode = fields.Selection([
        ('off', 'Off'),
        ('flexible', 'Flexible'),
        ('full', 'Full'),
        ('strict', 'Full (strict)')
    ], string='SSL Mode')
    auto_minify = fields.Boolean(string='Auto Minify')
    bot_fight_mode = fields.Boolean(string='Bot Fight Mode')
    website_id = fields.Many2one('website', string='Website')

class CloudflareRateLimit(models.Model):
    _name = 'cloudflare.rate.limit'
    _description = 'Cloudflare Rate Limit'

    name = fields.Char(string='Name', required=True)
    match_criteria = fields.Char(string='Matching Criteria')
    mitigation_action = fields.Selection([
        ('block', 'Block'),
        ('challenge', 'Challenge'),
        ('js_challenge', 'JS Challenge'),
        ('managed_challenge', 'Managed Challenge')
    ], string='Mitigation Action')
    website_id = fields.Many2one('website', string='Website')

class CloudflareCacheRule(models.Model):
    _name = 'cloudflare.cache.rule'
    _description = 'Cloudflare Cache Rule'

    name = fields.Char(string='Name', required=True)
    edge_cache_ttl = fields.Integer(string='Edge Cache TTL (seconds)')
    bypass_rules = fields.Text(string='Bypass Rules')
    website_id = fields.Many2one('website', string='Website')

class CloudflareZeroTrustPolicy(models.Model):
    _name = 'cloudflare.zero.trust.policy'
    _description = 'Cloudflare Zero Trust Policy'

    name = fields.Char(string='Name', required=True)
    policy_action = fields.Selection([
        ('allow', 'Allow'),
        ('block', 'Block'),
        ('bypass', 'Bypass')
    ], string='Action')
    idps = fields.Char(string='Identity Providers (IdPs)')
    website_id = fields.Many2one('website', string='Website')
