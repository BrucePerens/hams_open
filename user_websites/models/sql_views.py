# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields, tools


class UserWebsitesPublicDirectoryView(models.Model):
    _name = "user_websites.public_directory_view"
    _description = "Community Directory SQL View"
    _auto = False

    user_id = fields.Many2one("res.users", string="User", readonly=True)
    name = fields.Char(string="Name", readonly=True)
    website_slug = fields.Char(string="Slug", readonly=True)
    view_count = fields.Integer(string="Total Views", readonly=True)

    def init(self):
        tools.drop_view_if_exists(self.env.cr, self._table)
        with self.env.cr.savepoint():
            self.env.cr.execute(
                """
                CREATE OR REPLACE VIEW user_websites_public_directory_view AS (
                SELECT
                    u.id as id,
                    u.id as user_id,
                    p.name as name,
                    u.website_slug as website_slug,
                    COALESCE((SELECT SUM(view_count) FROM website_page WHERE owner_user_id = u.id), 0) +
                    COALESCE((SELECT SUM(view_count) FROM blog_post WHERE owner_user_id = u.id), 0) as view_count
                FROM res_users u
                JOIN res_partner p ON u.partner_id = p.id
                WHERE u.active IS TRUE
                AND u.website_slug IS NOT NULL
                AND u.website_slug != ''
                AND u.privacy_show_in_directory IS TRUE
                AND u.is_service_account IS NOT TRUE
            )
        """
        )


class UserWebsitesContentRoutingView(models.Model):
    _name = "user_websites.content_routing_view"
    _description = "Content Routing SQL View"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _auto = False

    res_model = fields.Char(string="Model", readonly=True)
    res_id = fields.Integer(string="Resource ID", readonly=True)
    website_slug = fields.Char(string="Slug", readonly=True)

    def init(self):
        tools.drop_view_if_exists(self.env.cr, self._table)
        with self.env.cr.savepoint():
            self.env.cr.execute(
                """
                CREATE OR REPLACE VIEW user_websites_content_routing_view AS (
                SELECT
                    u.id as id,
                    'res.users' as res_model,
                    u.id as res_id,
                    u.website_slug as website_slug
                FROM res_users u
                WHERE u.active IS TRUE
                AND u.website_slug IS NOT NULL
                AND u.website_slug != ''

                UNION ALL

                SELECT
                    id + 1000000 as id,
                    'user.websites.group' as res_model,
                    id as res_id,
                    website_slug as website_slug
                FROM user_websites_group
                WHERE website_slug IS NOT NULL
                AND website_slug != ''
            )
        """
        )


class UserWebsitesWeeklyDigestView(models.Model):
    _name = "user_websites.weekly_digest_view"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _description = "Weekly Digest SQL View"
    _auto = False

    partner_id = fields.Many2one("res.partner", string="Subscriber", readonly=True)
    author_name = fields.Char(string="Author", readonly=True)
    owner_model = fields.Char(string="Owner Model", readonly=True)
    owner_record_id = fields.Integer(string="Owner ID", readonly=True)
    post_ids_string = fields.Char(string="Post IDs", readonly=True)
    first_post_id = fields.Integer(string="First Post ID", readonly=True)

    def init(self):
        tools.drop_view_if_exists(self.env.cr, self._table)
        with self.env.cr.savepoint():
            self.env.cr.execute(
                """
                CREATE OR REPLACE VIEW user_websites_weekly_digest_view AS (
                SELECT
                    row_number() OVER () as id,
                    'Weekly Digest SQL View'::varchar as name,
                    f.partner_id as partner_id,
                    p.name as author_name,
                    'res.partner' as owner_model,
                    u.partner_id as owner_record_id,
                    string_agg(pst.id::text, ',') as post_ids_string,
                    min(pst.id) as first_post_id
                FROM mail_followers f
                JOIN res_users u ON (f.res_model = 'res.partner' AND f.res_id = u.partner_id)
                JOIN res_partner p ON u.partner_id = p.id
                JOIN blog_post pst ON (pst.owner_user_id = u.id)
                WHERE pst.is_published IS TRUE
                AND pst.create_date >= now() - interval '7 days'
                GROUP BY f.partner_id, u.partner_id, p.name

                UNION ALL

                SELECT
                    row_number() OVER () + 5000000 as id,
                    'Weekly Digest SQL View'::varchar as name,
                    f.partner_id as partner_id,
                    g.name as author_name,
                    'user.websites.group' as owner_model,
                    g.id as owner_record_id,
                    string_agg(pst.id::text, ',') as post_ids_string,
                    min(pst.id) as first_post_id
                FROM mail_followers f
                JOIN user_websites_group g ON (f.res_model = 'user.websites.group' AND f.res_id = g.id)
                JOIN blog_post pst ON (pst.user_websites_group_id = g.id)
                WHERE pst.is_published IS TRUE
                AND pst.create_date >= now() - interval '7 days'
                GROUP BY f.partner_id, g.id, g.name
            )
        """
        )


class UserWebsitesDbFunctions(models.AbstractModel):
    _name = "user_websites.db_functions"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _description = "User Websites DB Functions"

    def init(self):
        with self.env.cr.savepoint():
            self.env.cr.execute(
                """
                CREATE OR REPLACE FUNCTION increment_strike_count(tbl_name text, rec_id integer)
            RETURNS void AS $$
            BEGIN
                IF tbl_name = 'res_users' THEN
                    PERFORM id FROM res_users WHERE id = rec_id FOR NO KEY UPDATE;
                    UPDATE res_users SET violation_strike_count = violation_strike_count + 1 WHERE id = rec_id;
                ELSIF tbl_name = 'user_websites_group' THEN
                    PERFORM id FROM user_websites_group WHERE id = rec_id FOR NO KEY UPDATE;
                    UPDATE user_websites_group SET violation_strike_count = violation_strike_count + 1 WHERE id = rec_id;
                END IF;
            END;
            $$ LANGUAGE plpgsql;
        """
        )
