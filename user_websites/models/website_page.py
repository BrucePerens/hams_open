# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import os
import redis
import logging
import re
import odoo
import json
from lxml import etree
from odoo import models, fields, api, _
from odoo.exceptions import ValidationError, AccessError
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    invalidate_model_cache,
)

_logger = logging.getLogger(__name__)

REDIS_HOST = os.environ.get("REDIS_HOST", "redis")
REDIS_PORT = int(os.environ.get("REDIS_PORT", "6379"))
redis_pool = redis.ConnectionPool(
    host=REDIS_HOST,
    port=REDIS_PORT,
    db=0,
    decode_responses=True,
    socket_timeout=1.0,
    socket_connect_timeout=1.0,
)
redis_client = redis.Redis(connection_pool=redis_pool)


class WebsitePage(models.Model):
    _name = "website.page"
    _inherit = ["website.page", "user_websites.owned.mixin"]

    view_count = fields.Integer(
        string="View Count", default=0, help="Privacy-friendly tracking of page views."
    )

    _url_website_uniq = models.Constraint(
        "UNIQUE(url, website_id)", "The page URL must be unique per website!"
    )

    def _invalidate_cloudflare_cache(self):
        """Soft-dependency hook to purge the global Cache-Tag at the edge."""
        if "cloudflare.purge.queue" in self.env and not odoo.tools.config.get("test_enable"):
            # ADR 0078: Pre-fetch related fields to prevent N+1 queries in the loop
            self.mapped("owner_user_id.website_slug")
            self.mapped("user_websites_group_id.website_slug")

            tags = set()
            for rec in self:
                if rec.owner_user_id and rec.owner_user_id.website_slug:
                    tags.add(f"site-{rec.owner_user_id.website_slug}")
                elif (
                    rec.user_websites_group_id
                    and rec.user_websites_group_id.website_slug
                ):
                    tags.add(f"site-{rec.user_websites_group_id.website_slug}")
            if tags:
                try:
                    svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                        "cloudflare.user_cloudflare_service"
                    )
                    self.env["cloudflare.purge.queue"].with_user(svc_uid).enqueue_tags(
                        list(tags)
                    )
                except AccessError as e:
                    if "Service Account" in str(e):
                        logging.getLogger(__name__).debug("Cloudflare purge skipped: %s", e)
                    else:
                        logging.getLogger(__name__).exception("Access error during Cloudflare purge")
                except Exception: # audit-ignore-catch-all
                    logging.getLogger(__name__).exception("Fatal error during Cloudflare purge")

    @api.model
    def _sanitize_user_arch(self, arch_content):
        # [@ANCHOR: website_page_sanitize_arch]
        # Verified by [@ANCHOR: test_website_page_sanitize_arch]
        if not arch_content:
            return arch_content, False
        try:
            parser = etree.XMLParser(recover=True)
            root = etree.fromstring(f"<root>{arch_content}</root>", parser=parser)

            was_modified = False

            # Strip script, iframe, object, embed entirely
            for tag in ["script", "iframe", "object", "embed"]:
                for elem in root.xpath(f'//*[local-name()="{tag}"]'):
                    elem.getparent().remove(elem)
                    was_modified = True

            # Strip all QWeb execution, inline JS directives, and javascript URIs
            dangerous_prefixes = ("t-", "on")
            # ADR-0102: Explicitly allow safe QWeb directives while blocking SSTI vectors
            # Finite whitelist of allowed t-att-* attributes to minimize attack surface.
            ALLOWED_T_DIRECTIVES = {
                "t-name", "t-call", "t-set", "t-value", "t-out", "t-esc",
                "t-if", "t-elif", "t-else", "t-foreach", "t-as",
                "t-att-class", "t-att-style", "t-att-src", "t-att-href",
                "t-att-alt", "t-att-title", "t-att-name", "t-att-value",
                "t-att-data-target", "t-att-data-bs-target",
                "t-att-data-bs-toggle", "t-att-role", "t-att-aria-label",
                "t-att-placeholder", "t-att-id", "t-att-checked", "t-att-selected"
            }

            for elem in root.xpath("//*"):
                for attr in list(elem.attrib.keys()):
                    attr_lower = attr.lower()
                    # Prevent XML namespace bypasses
                    if attr_lower.startswith("xmlns") or ":" in attr_lower:
                        del elem.attrib[attr]
                        was_modified = True
                        continue
                    val = elem.attrib[attr]
                    # Block all dangerous URI schemes (data, vbscript, javascript)
                    if attr_lower in (
                        "href",
                        "src",
                        "content",
                        "formaction",
                        "action",
                    ) and re.match(
                        r"^\s*(javascript|data|vbscript):", val, re.IGNORECASE
                    ):
                        del elem.attrib[attr]
                        elem.attrib[f"data-blocked-{attr}"] = val
                        was_modified = True
                    elif attr_lower.startswith(dangerous_prefixes):
                        if attr_lower not in ALLOWED_T_DIRECTIVES:
                            del elem.attrib[attr]
                            elem.attrib[f"data-blocked-{attr}"] = val
                            was_modified = True
                        else:
                            # Additional expression-level check for SSTI vectors
                            # Blocks .sudo(), .with_user(), eval(), exec(), and dunder methods
                            if re.search(r"\.(sudo|with_user|with_context|with_env|env)\s*\(|__|\b(eval|exec|getattr|setattr)\b", val):
                                del elem.attrib[attr]
                                elem.attrib[f"data-blocked-ssti-{attr}"] = val
                                was_modified = True

            # Return inner HTML of root without the wrapper
            # Correctly handle text nodes and tail text to prevent data loss (Bug Fix)
            full_string = etree.tostring(root, encoding="unicode", method="xml")
            # Remove the <root> and </root> tags added for parsing
            start_tag_end = full_string.find(">") + 1
            end_tag_start = full_string.rfind("</root>")
            sanitized_content = full_string[start_tag_end:end_tag_start]

            return sanitized_content, was_modified
        except Exception: # audit-ignore-catch-all
            _logger.exception("Failed to sanitize user arch")
            return "<div>Sanitization Error</div>", True

    @api.model
    def _trigger_malicious_arch_violation(self, vals, records=None):
        """Creates an automated violation report and issues a strike when malicious SSTI/XSS is stripped."""
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )

        owner_id = vals.get("owner_user_id")
        group_id = vals.get("user_websites_group_id")

        if not owner_id and not group_id and records:
            owner_id = (
                records[0].owner_user_id.id if records[0].owner_user_id else False
            )
            group_id = (
                records[0].user_websites_group_id.id
                if records[0].user_websites_group_id
                else False
            )

        url = vals.get("url")
        if not url and records:
            url = records[0].url

        target_url = url or "/unknown-page"
        reporter_id = self.env.user.id

        existing = (
            self.env["content.violation.report"]
            .with_user(svc_uid)
            .search(
                [
                    ("target_url", "=", target_url),
                    ("reported_by_user_id", "=", reporter_id),
                ],
                limit=1,
            )
        )

        if existing:
            existing.with_user(svc_uid).action_take_action_and_strike()
        else:
            report = (
                self.env["content.violation.report"]
                .with_user(svc_uid)
                .create(
                    {
                        "target_url": target_url,
                        "description": "🚨 AUTOMATED SECURITY ALERT: The system detected and neutralized malicious code (SSTI/XSS) attempting to execute on this page. The attacker attempted to inject forbidden <script> tags, IFrames, or t-* QWeb evaluation directives. The payload was stripped, but the user account may be compromised or acting maliciously.",
                        "content_owner_id": owner_id,
                        "content_group_id": group_id,
                        "reported_by_user_id": reporter_id,
                    }
                )
            )
            # Immediately strike the offending account (fulfilling "at least the level of a content-rule violation")
            report.with_user(svc_uid).action_take_action_and_strike()

    @api.model
    @distributed_cache()
    def _get_page_id_by_url(self, url, website_id, override_svc_uid=None):
        if not url:
            return False
        svc_uid = override_svc_uid or self.env[
            "zero_sudo.security.utils"
        ]._get_service_uid("user_websites.user_user_websites_service_account")
        page = self.with_user(svc_uid).search(
            [
                ("url", "=", url),
                ("website_published", "=", True),
                "|",
                ("website_id", "=", False),
                ("website_id", "=", website_id),
            ],
            limit=1,
        )
        return page.id if page else False

    @api.model_create_multi
    def create(self, vals_list):
        # Verified by [@ANCHOR: test_site_creation_performance_scaling]
        # 0. Sanitize arch to prevent Stored XSS
        if not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            for vals in vals_list:
                if vals.get("arch"):
                    sanitized_arch, modified = self._sanitize_user_arch(vals["arch"])
                    vals["arch"] = sanitized_arch
                    if modified:
                        self._trigger_malicious_arch_violation(vals)

        # 1. Enforce Mixin Security
        self._check_proxy_ownership_create(vals_list)

        if not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            allowed = {
                "name",
                "url",
                "arch",
                "is_published",
                "website_published",
                "type",
                "owner_user_id",
                "user_websites_group_id",
                "key",
                "website_id",
                "view_count",
            }
            for vals in vals_list:
                for k in list(vals.keys()):
                    if k not in allowed:
                        del vals[k]

        # [@ANCHOR: website_page_quota_check]
        # Verified by [@ANCHOR: test_page_quota_limit]
        # Verified by [@ANCHOR: test_page_limits]
        # 2. Quota Limit Check
        owner_ids = [
            vals.get("owner_user_id") for vals in vals_list if vals.get("owner_user_id")
        ]
        if owner_ids:
            unique_owner_ids = list(set(owner_ids))
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )
            users = self.env["res.users"].with_user(svc_uid).browse(unique_owner_ids)
            user_limits = {user.id: user._get_page_limit() for user in users}

            existing_counts = {u_id: 0 for u_id in unique_owner_ids}
            page_counts = (
                self.env["website.page"]
                .with_user(svc_uid)
                ._read_group(
                    [("owner_user_id", "in", unique_owner_ids)],
                    ["owner_user_id"],
                    ["__count"],
                )
            )
            for owner, count in page_counts:
                existing_counts[owner.id] = count

            batch_counts = {u_id: 0 for u_id in unique_owner_ids}
            for vals in vals_list:
                o_id = vals.get("owner_user_id")
                if o_id:
                    batch_counts[o_id] += 1

            for o_id in unique_owner_ids:
                if existing_counts[o_id] + batch_counts[o_id] > user_limits[o_id]:
                    raise ValidationError(
                        _("You have reached your limit of %s website pages.")
                        % user_limits[o_id]
                    )

        group_ids = [
            vals.get("user_websites_group_id")
            for vals in vals_list
            if vals.get("user_websites_group_id")
        ]
        if group_ids:
            unique_group_ids = list(set(group_ids))
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )
            global_limit = int(
                self.env["zero_sudo.security.utils"]._get_system_param(
                    "user_websites.global_website_page_limit", 100
                )
            )

            existing_group_counts = {g_id: 0 for g_id in unique_group_ids}
            group_counts = (
                self.env["website.page"]
                .with_user(svc_uid)
                ._read_group(
                    [("user_websites_group_id", "in", unique_group_ids)],
                    ["user_websites_group_id"],
                    ["__count"],
                )
            )
            for group, count in group_counts:
                existing_group_counts[group.id] = count

            batch_group_counts = {g_id: 0 for g_id in unique_group_ids}
            for vals in vals_list:
                g_id = vals.get("user_websites_group_id")
                if g_id:
                    batch_group_counts[g_id] += 1

            for g_id in unique_group_ids:
                if (
                    existing_group_counts[g_id] + batch_group_counts[g_id]
                    > global_limit
                ):
                    raise ValidationError(
                        _("This group has reached its limit of %s website pages.")
                        % global_limit
                    )

        # 3. Apply Service Account to safely bypass standard ir.ui.view creation restrictions
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        # ADR-0001: All service account mutations must include appropriate context
        self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
        records = super(WebsitePage, self_svc).create(vals_list)
        records._invalidate_cloudflare_cache()
        return records

    def check_access_rule(self, operation):
        # Verified by [@ANCHOR: test_acl_overhead_loop_elimination]
        """
        Proactively catch write/unlink access violations for standard users on pages they don't own.
        This prevents Odoo's core `ir.rule` engine from generating massive amounts of INFO log spam
        (cybercrud) every time an internal user visits a public page and the frontend evaluates
        if they should see the 'Edit' button.
        """
        if operation in ("write", "unlink") and not self.env.su and self:
            if self.env.user.has_group(
                "user_websites.group_user_websites_user"
            ) and not self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            ):
                user_id = self.env.user.id

                # ADR-0022: Pre-fetch group memberships to prevent N+1 lazy-load queries in the loop
                group_ids = self.mapped("user_websites_group_id").ids
                member_map = {}
                if group_ids:
                    svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                        "user_websites.user_user_websites_service_account"
                    )
                    groups = (
                        self.env["user.websites.group"]
                        .with_user(svc_uid)
                        .browse(group_ids)
                    )
                    for g in groups:
                        member_map[g.id] = set(g.member_ids.ids)

                for page in self:
                    is_owner = page.owner_user_id.id == user_id
                    is_group_member = (
                        page.user_websites_group_id
                        and user_id
                        in member_map.get(page.user_websites_group_id.id, set())
                    )
                    if not is_owner and not is_group_member:
                        raise AccessError(
                            _(
                                "Access Denied: You do not have permission to modify this page."
                            )
                        )
        return super(WebsitePage, self).check_access_rule(operation)

    def write(self, vals):
        # Verified by [@ANCHOR: test_tenant_view_isolation]
        self.check_access("write")
        self._check_proxy_ownership_write(vals)

        if not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            allowed = {
                "name",
                "url",
                "arch",
                "is_published",
                "website_published",
                "type",
                "owner_user_id",
                "user_websites_group_id",
                "key",
                "website_id",
                "view_count",
            }
            for k in list(vals.keys()):
                if k not in allowed:
                    del vals[k]

        if "arch" in vals and not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            sanitized_arch, modified = self._sanitize_user_arch(vals["arch"])
            vals["arch"] = sanitized_arch
            if modified:
                self._trigger_malicious_arch_violation(vals, records=self)

        # Identify URLs to invalidate before mutating
        pages_to_invalidate = [p.url for p in self if p.url]

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        # ADR-0001: All service account mutations must include appropriate context
        self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
        res = super(WebsitePage, self_svc).write(vals)

        # Targeted DB NOTIFY invalidation (O(1) line eviction instead of global clear)
        if "url" in vals or "website_published" in vals or "is_published" in vals:
            utils = self.env["zero_sudo.security.utils"]
            urls_to_notify = list(pages_to_invalidate)
            if "url" in vals and vals["url"] not in urls_to_notify:
                urls_to_notify.append(vals["url"])
            if urls_to_notify:
                utils._notify_cache_invalidation("website.page", urls_to_notify)
                invalidate_model_cache(self.env, self._name)
                payload = json.dumps({"model": self._name})
                self.env.cr.execute(
                    "SELECT pg_notify(%s, %s)",
                    ("distributed_cache_invalidation", payload),
                )

        self._invalidate_cloudflare_cache()
        return res

    def unlink(self):
        self.check_access("unlink")

        pages_to_invalidate = [p.url for p in self if p.url]
        self._invalidate_cloudflare_cache()

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        # ADR-0001: All service account mutations must include appropriate context
        self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
        res = super(WebsitePage, self_svc).unlink()

        utils = self.env["zero_sudo.security.utils"]
        if pages_to_invalidate:
            utils._notify_cache_invalidation("website.page", pages_to_invalidate)
            invalidate_model_cache(self.env, self._name)
            payload = json.dumps({"model": self._name})
            self.env.cr.execute(
                "SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload)
            )

        return res

    @api.model
    def _flush_redis_view_counters(self):
        """
        Cron job to flush Redis view counters to the PostgreSQL database.
        Uses _trigger() for batching if there are too many keys (ADR-0022).
        """
        try:
            db_name = self.env.cr.dbname
            cursor, keys = redis_client.scan(
                cursor=0, match=f"views:{db_name}:page:*", count=1000
            )
        except redis.exceptions.RedisError as e:
            _logger.error(f"Failed to connect to Redis for view counter flush: {e}")
            return

        if not keys:
            return

        pipe = redis_client.pipeline()
        for key in keys:
            pipe.get(key)

        results = pipe.execute()

        updates = []
        for i, key in enumerate(keys):
            val = results[i]
            if val:
                try:
                    page_id = int(key.split(":")[-1])
                    increment = int(val)
                    updates.append((increment, page_id))
                except ValueError:
                    _logger.warning('Failed to parse Redis key or value for view count.')

        if updates:
            try:
                for inc, pid in updates:
                    self.env.cr.execute(
                        "UPDATE website_page SET view_count = COALESCE(view_count, 0) + %s WHERE id = %s",
                        (inc, pid),
                    )

                # RACE CONDITION FIX: Delete from Redis first. If PostgreSQL commits, state is perfect.
                # If PostgreSQL fails and rolls back, we lose views (acceptable), avoiding double-counting (unacceptable).
                del_pipe = redis_client.pipeline()
                for i, key in enumerate(keys):
                    val = results[i]
                    if val:
                        del_pipe.decrby(key, int(val))
                del_pipe.execute()

                if not odoo.tools.config.get("test_enable"):
                    self.env.cr.commit()
            except Exception: # audit-ignore-catch-all
                if not odoo.tools.config.get("test_enable"):
                    self.env.cr.rollback()
                _logger.exception("Error updating PostgreSQL view counts")

        if cursor != 0:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_user_websites_service_account"
            )
            cron = self.env.ref("user_websites.ir_cron_flush_view_counters", raise_if_not_found=False)
            if cron:
                cron.with_user(svc_uid).with_context(mail_notrack=True)._trigger()
