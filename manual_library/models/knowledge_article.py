# -*- coding: utf-8 -*-
from odoo import models, fields, api, _
from odoo.exceptions import ValidationError
import re
import unicodedata


class KnowledgeArticle(models.Model):
    """
    Clean-room implementation of the knowledge.article API for Odoo Community.
    """

    _name = "knowledge.article"
    _description = "Manual Article"
    _inherit = ["mail.thread", "mail.activity.mixin", "website.published.mixin", "website.multi.mixin"]
    _order = "sequence, id"

    # --- API Interoperability Fields (Strict Matching) ---
    name = fields.Char(
        string="Title", required=True, translate=True, tracking=True, index="trigram"
    )
    body = fields.Html(string="Body", sanitize=True)
    parent_id = fields.Many2one(
        "knowledge.article", string="Parent Article", ondelete="restrict", index=True
    )
    child_ids = fields.One2many(
        "knowledge.article", "parent_id", string="Child Articles"
    )
    sequence = fields.Integer(
        string="Sequence", default=10, help="Order of presentation."
    )

    # is_published is provided by website.published.mixin, but we declare it explicitly for API clarity
    is_published = fields.Boolean(string="Is Published", default=False, tracking=True)
    icon = fields.Char(
        string="Article Icon", help="Emoji or icon class for UI representation."
    )
    active = fields.Boolean(string="Active", default=True)

    website_id = fields.Many2one("website", string="Website", ondelete="cascade")

    internal_permission = fields.Selection(
        [("read", "Read Only"), ("write", "Read & Write"), ("none", "No Access")],
        string="Internal Permission",
        default="read",
        tracking=True,
    )

    member_ids = fields.Many2many(
        "res.users",
        "knowledge_article_member_rel",
        "article_id",
        "user_id",
        string="Shared Members",
    )

    # --- Custom Implementation Fields ---
    helpful_count = fields.Integer(string="Helpful Votes", default=0, readonly=True)
    unhelpful_count = fields.Integer(string="Unhelpful Votes", default=0, readonly=True)

    # --- Constraints ---
    @api.constrains("parent_id")
    def _check_hierarchy(self):
        # [@ANCHOR: manual_check_hierarchy]
        # See story_manual_hierarchy and journey_admin_managing
        # Verified by [@ANCHOR: test_manual_check_hierarchy]
        """Prevent circular references in the article tree."""
        if self._has_cycle():
            raise ValidationError(_("Error! You cannot create recursive articles."))

    # --- Compute Methods ---
    @api.depends("name")
    def _compute_website_url(self):
        # [@ANCHOR: manual_compute_website_url]
        # See story_manual_url_generation and journey_admin_managing
        # Verified by [@ANCHOR: test_manual_url_slug_generation]
        """Override from website.published.mixin to provide a proper slug."""
        super(KnowledgeArticle, self)._compute_website_url()
        for article in self:
            if article.id:
                s = str(article.name or "").strip().lower()
                s = (
                    unicodedata.normalize("NFKD", s)
                    .encode("ascii", "ignore")
                    .decode("utf-8")
                )
                safe_name = re.sub(r"[^a-z0-9]+", "-", s).strip("-")
                article.website_url = f"/manual/{article.id}-{safe_name}"
            else:
                article.website_url = ""
