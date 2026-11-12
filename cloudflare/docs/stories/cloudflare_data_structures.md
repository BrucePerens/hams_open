# Data structures

### `WebsitePage` [@ANCHOR: cloudflare:cloudflare_bridge_website_page]

**Purpose and location.** Extends the Odoo model ["website.page", "cloudflare.purge.mixin"] with the fields `name` and the methods `write`, `unlink`. Defined in `cloudflare/models/bridge.py:6` as an Odoo model; bases: `models.Model`. (model `website.page`; inherits ["website.page", "cloudflare.purge.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name |

**Class attributes.** `_name = "website.page"`; `_inherit = ["website.page", "cloudflare.purge.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `write`, `unlink`.

**See also:** test `test_blog_post_write_enqueues_a_purge (tests/test_bridge_and_misc.py)`.

### `BlogPost` [@ANCHOR: cloudflare:cloudflare_bridge_blog_post]

**Purpose and location.** Extends the Odoo model ["blog.post", "cloudflare.purge.mixin"] with the fields `name` and the methods `write`, `unlink`. Defined in `cloudflare/models/bridge.py:24` as an Odoo model; bases: `models.Model`. (model `blog.post`; inherits ["blog.post", "cloudflare.purge.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name |

**Class attributes.** `_name = "blog.post"`; `_inherit = ["blog.post", "cloudflare.purge.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `write`, `unlink`.

**See also:** test `test_blog_post_write_enqueues_a_purge (tests/test_bridge_and_misc.py)`.

### `WebsiteMenu` [@ANCHOR: cloudflare:cloudflare_bridge_website_menu]

**Purpose and location.** Extends the Odoo model ["website.menu", "cloudflare.purge.mixin"] with the fields `name` and the methods `write`, `unlink`. Defined in `cloudflare/models/bridge.py:52` as an Odoo model; bases: `models.Model`. (model `website.menu`; inherits ["website.menu", "cloudflare.purge.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name |

**Class attributes.** `_name = "website.menu"`; `_inherit = ["website.menu", "cloudflare.purge.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `write`, `unlink`.

**See also:** test `test_blog_post_write_enqueues_a_purge (tests/test_bridge_and_misc.py)`.

### `ProductTemplate` [@ANCHOR: cloudflare:cloudflare_bridge_product_template]

**Purpose and location.** Extends the Odoo model ["product.template", "cloudflare.purge.mixin"] with the fields `name` and the methods `write`, `unlink`. Defined in `cloudflare/models/bridge.py:69` as an Odoo model; bases: `models.Model`. (model `product.template`; inherits ["product.template", "cloudflare.purge.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name |

**Class attributes.** `_name = "product.template"`; `_inherit = ["product.template", "cloudflare.purge.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `write`, `unlink`.

**See also:** test `test_blog_post_write_enqueues_a_purge (tests/test_bridge_and_misc.py)`.

### `CloudflareZoneSettings` [@ANCHOR: cloudflare:cloudflare_cloudflare_features_cloudflare_zone_settings]

**Purpose and location.** Model description: Cloudflare Zone Settings. Defined in `cloudflare/models/cloudflare_features.py:8` as an Odoo model; bases: `models.Model`. (model `cloudflare.zone.settings`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |
| `ssl_mode` | `Selection` | SSL Mode (values `off`, `flexible`, `full`, `strict`) |
| `auto_minify` | `Boolean` | Auto Minify: Automatically minify HTML, CSS, and JS. |
| `bot_fight_mode` | `Boolean` | Bot Fight Mode: Challenge bad bots. |
| `website_id` | `Many2one` | Website (comodel `website`) |

**Class attributes.** `_name = "cloudflare.zone.settings"`; `_description = "Cloudflare Zone Settings"`; `_check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflareRateLimit` [@ANCHOR: cloudflare:cloudflare_cloudflare_features_cloudflare_rate_limit]

**Purpose and location.** Model description: Cloudflare Rate Limit. Defined in `cloudflare/models/cloudflare_features.py:31` as an Odoo model; bases: `models.Model`. (model `cloudflare.rate.limit`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |
| `match_criteria` | `Char` | Matching Criteria |
| `mitigation_action` | `Selection` | Mitigation Action (values `block`, `challenge`, `js_challenge`, `managed_challenge`) |
| `website_id` | `Many2one` | Website (comodel `website`) |

**Class attributes.** `_name = "cloudflare.rate.limit"`; `_description = "Cloudflare Rate Limit"`; `_check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflareCacheRule` [@ANCHOR: cloudflare:cloudflare_cloudflare_features_cloudflare_cache_rule]

**Purpose and location.** Model description: Cloudflare Cache Rule. Defined in `cloudflare/models/cloudflare_features.py:53` as an Odoo model; bases: `models.Model`. (model `cloudflare.cache.rule`; default order "sequence, id".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |
| `edge_cache_ttl` | `Integer` | Edge Cache TTL (seconds): 0 on a cache rule means: defer to the origin's Cloudflare-CDN-Cache-Control header. A positive value overrides the origin. Ignored on a bypass rule. |
| `bypass_rules` | `Text` | Bypass Rules |
| `sequence` | `Integer` | Sequence (default=`10`) |
| `active` | `Boolean` | Active (default=`True`) |
| `action` | `Selection` | Action: Bypass rules are always pushed ahead of cache rules, whatever their sequence, so pages are never cacheable without the bypass in front of them. (values `bypass`, `cache`; required=`True`; default=`"cache"`) |
| `expression` | `Text` | Expression: Cloudflare Ruleset Engine expression, e.g. (http.cookie contains "session_id"). 'true' matches every request. (default=`"true"`) |
| `website_id` | `Many2one` | Website (comodel `website`) |

**Class attributes.** `_name = "cloudflare.cache.rule"`; `_description = "Cloudflare Cache Rule"`; `_order = "sequence, id"`; `_check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_plan_cache_push`, `action_push_to_cloudflare`.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflareZeroTrustPolicy` [@ANCHOR: cloudflare:cloudflare_cloudflare_features_cloudflare_zero_trust_policy]

**Purpose and location.** Model description: Cloudflare Zero Trust Policy. Defined in `cloudflare/models/cloudflare_features.py:132` as an Odoo model; bases: `models.Model`. (model `cloudflare.zero.trust.policy`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |
| `policy_action` | `Selection` | Action (values `allow`, `block`, `bypass`) |
| `idps` | `Char` | Identity Providers (IdPs) |
| `website_id` | `Many2one` | Website (comodel `website`) |

**Class attributes.** `_name = "cloudflare.zero.trust.policy"`; `_description = "Cloudflare Zero Trust Policy"`; `_check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflareConfigBackup` [@ANCHOR: cloudflare:cloudflare_config_backup_cloudflare_config_backup]

**Purpose and location.** Model description: Cloudflare Configuration Backup. Defined in `cloudflare/models/config_backup.py:6` as an Odoo model; bases: `models.Model`. (model `cloudflare.config.backup`; default order "create_date desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Backup Name (required=`True`) |
| `phase` | `Char` | Ruleset Phase (default=`"http_request_firewall_custom"`) |
| `raw_json` | `Text` | Raw JSON Payload (required=`True`) |
| `create_date` | `Datetime` | Backed Up On (readonly=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; default=`lambda self: self.env["website"].get_current_websi`) |

**Class attributes.** `_name = "cloudflare.config.backup"`; `_description = "Cloudflare Configuration Backup"`; `_order = "create_date desc"`; `_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "The backup name cannot be empty."
    )`; `_json_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(raw_json)) > 0)", "The JSON payload cannot be empty."
`; `_name_website_uniq = models.Constraint(
        "UNIQUE(name, website_id)",
        "A backup with this name already exis`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflareConfigManager` [@ANCHOR: cloudflare:cloudflare_config_manager_cloudflare_config_manager]

**Purpose and location.** Model description: Cloudflare Configuration Manager. Defined in `cloudflare/models/config_manager.py:51` as an Odoo model; bases: `models.AbstractModel`. (model `cloudflare.config.manager`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |

**Class attributes.** `_name = "cloudflare.config.manager"`; `_description = "Cloudflare Configuration Manager"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_trigger_edge_purge_static_assets`, `initialize_cloudflare_state`, `_check_waf_caller_authorized`, `action_pull_waf_rules`, `action_push_waf_rules`, `_build_cache_ruleset_rules`, `plan_cache_rules`, `action_push_cache_rules`.

**See also:** test `test_trigger_edge_purge_static_assets_enqueues_on_mtime_increase (tests/test_bridge_and_misc.py)`.

### `CloudflareDnsPushWizard` [@ANCHOR: cloudflare:cloudflare_dns_push_wizard_cloudflare_dns_push_wizard]

**Purpose and location.** Two steps: show the plan (reads only), then apply exactly that plan. Defined in `cloudflare/models/dns_push_wizard.py:10` as an Odoo model; bases: `models.TransientModel`. (model `cloudflare.dns.push.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `record_ids` | `Many2many` | Records (comodel `cloudflare.dns.record`; default=`lambda self: self.env["cloudflare.dns.record"].bro`) |
| `state` | `Selection` | no label or help in the source (values `draft`, `planned`, `done`; default=`"draft"`) |
| `plan_text` | `Text` | Plan (readonly=`True`) |
| `plan_hash` | `Char` | no label or help in the source (readonly=`True`) |
| `plan_writes` | `Integer` | no label or help in the source (readonly=`True`) |
| `result_text` | `Text` | Result (readonly=`True`) |

**Class attributes.** `_name = "cloudflare.dns.push.wizard"`; `_description = "Cloudflare DNS Push"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_reopen`, `action_plan`, `action_apply`.

**See also:** test `test_plan_changes_nothing_and_stale_hash_is_refused (tests/test_cache_rule_push.py)`.

### `CloudflareDNSRecord` [@ANCHOR: cloudflare:cloudflare_dns_record_cloudflare_dnsrecord]

**Purpose and location.** One DNS record that Odoo is the source of truth for.  A push (`dns_push_plan` then `dns_push_apply`) creates and updates only what Odoo has a row for. It never overwrites a record it did not create (adopting an existing record only stores its id) and never touches a record Odoo has no row for. Removing or archiving a row removes nothing at Cloudflare. The one delete is the explicit "retire" flag on a row linked to its record by id. Defined in `cloudflare/models/dns_record.py:27` as an Odoo model; bases: `models.Model`. (model `cloudflare.dns.record`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name: The full name, lower case, no trailing dot (callbook.hams.com). (required=`True`) |
| `type` | `Selection` | Type (values `A`, `AAAA`, `CNAME`, `TXT`, `NS`; required=`True`) |
| `content` | `Char` | Content (required=`True`) |
| `proxied` | `Boolean` | Proxied: Only A, AAAA and CNAME records can be proxied. A nameserver name (glue) and every NS and TXT record are DNS-only. (default=`True`) |
| `website_id` | `Many2one` | Website: The website whose Cloudflare credentials push this row. Empty: the first website that has an API token. (comodel `website`) |
| `manage` | `Boolean` | Odoo manages it: Off: observe only. A push links the row to the matching record and reports differences, and writes nothing at Cloudflare. (default=`True`) |
| `cf_record_id` | `Char` | Cloudflare record id: Set by a push when it creates or adopts the record. To take over a record that already exists with other content, paste its id here: the next plan then shows the update. (index=`True`; copy=`False`) |
| `retire` | `Boolean` | Retire: Delete this row's own record at Cloudflare on the next push, if the row is managed and linked to it by id. The plan shows the delete first; afterwards the row is archived. Nothing else is ever deleted. (copy=`False`) |
| `active` | `Boolean` | no label or help in the source (default=`True`) |
| `comment` | `Char` | Comment: Why this record exists, for anything not obvious from its name (an ACME challenge CNAME, a DNS-only record and the reason). Sent to Cloudflare as the record's comment, which the dashboard shows. Empty means Odoo says nothing: a comment someone set at Cloudflare is left alone. (copy=`False`) |
| `cf_last_result` | `Char` | Last push result (readonly=`True`; copy=`False`) |

**Class attributes.** `_name = "cloudflare.dns.record"`; `_description = "Cloudflare DNS Record"`; `_check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `create`, `_onchange_type_clears_proxied`, `_check_record`, `_check_not_a_member_name`, `_require_ip`, `_check_glue`, `_check_dns_caller_authorized`, `_dns_credential_website`, `action_push_pending`, `dns_push_plan`, `_dns_problem_entry`, `_dns_zone_of`, `_dns_problems`, `_dns_member_name_problems`, `_dns_capacity_problems`, `dns_push_apply`.

**See also:** test `test_blog_post_write_enqueues_a_purge (tests/test_bridge_and_misc.py)`.

### `CloudflareRoutingDomain` [@ANCHOR: cloudflare:cloudflare_domain_cloudflare_routing_domain]

**Purpose and location.** Extends the Odoo model "edge.routing.domain" with the fields `cloudflare_hostname_id`, `ssl_status` and the methods `create`, `unlink`, `_get_website_mapping`, `_create_cloudflare_custom_hostname_batch`, `_delete_cloudflare_custom_hostname_batch`, `action_sync_ssl_status`. Defined in `cloudflare/models/domain.py:12` as an Odoo model; bases: `models.Model`. (extends `edge.routing.domain`.)

| Field | Type | Meaning |
|---|---|---|
| `cloudflare_hostname_id` | `Char` | ID returned by Cloudflare for management (readonly=`True`) |
| `ssl_status` | `Selection` | SSL Status (values `pending_validation`, `pending_issuance`, `pending_deployment`, `active`, `error`; default=`"pending_validation"`; readonly=`True`) |

**Class attributes.** `_inherit = "edge.routing.domain"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `create`, `unlink`, `_get_website_mapping`, `_create_cloudflare_custom_hostname_batch`, `_delete_cloudflare_custom_hostname_batch`, `action_sync_ssl_status`.

**See also:** test `test_blog_post_write_enqueues_a_purge (tests/test_bridge_and_misc.py)`.

### `CloudflareUtils` [@ANCHOR: cloudflare:cloudflare_edge_context_cloudflare_utils]

**Purpose and location.** Model description: Cloudflare Edge Context Utilities. Defined in `cloudflare/models/edge_context.py:11` as an Odoo model; bases: `models.AbstractModel`. (model `cloudflare.utils`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |

**Class attributes.** `_name = "cloudflare.utils"`; `_description = "Cloudflare Edge Context Utilities"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `get_current_website_id`, `get_request_context`.

**See also:** test `test_get_current_website_id_falls_back_to_get_current_website_outside_a_request (tests/test_bridge_and_misc.py)`.

### `CloudflareHostnamePendingDelete` [@ANCHOR: cloudflare:cloudflare_hostname_pending_delete_cloudflare_hostname_pending_delete]

**Purpose and location.** A custom hostname Cloudflare would not delete, kept so it can be retried.  `edge.routing.domain.unlink()` deletes the local row whatever Cloudflare says -- blocking it would make a domain undeletable whenever a token is revoked for good, which is worse than the leak it prevents. Bruce's decision (2026-09-19, `night_shift_questions/answered/ cloudflare-hostname-delete-failure-block-or-proceed-05a5144b.md`, "3 sounds good to me") is the third option on that question: **proceed with the local delete, but keep a retry record**, so the orphaned hostname on Cloudflare's zone has something pointing at it rather than only a log line nothing ever revisits.  **No credential is ever stored here.** The record holds the Cloudflare custom-hostname id, the domain name, the website the credentials come FROM, Cloudflare's own error text and some counters. The API token is fetched from the website at retry time, exactly as the original delete did. Defined in `cloudflare/models/hostname_pending_delete.py:27` as an Odoo model; bases: `models.Model`. (model `cloudflare.hostname.pending.delete`; default order "state, next_attempt_at, id".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (compute=`"_compute_name"`; readonly=`True`) |
| `cf_hostname_id` | `Char` | Cloudflare Hostname ID (required=`True`; index=`True`; readonly=`True`) |
| `domain_name` | `Char` | Domain (required=`True`; readonly=`True`) |
| `website_id` | `Many2one` | Website: The website whose Cloudflare credentials and zone this hostname belongs to. A pointer only -- no token is stored on this record. (comodel `website`; readonly=`True`; ondelete=`"set null"`) |
| `last_error` | `Char` | Last Cloudflare Error (readonly=`True`) |
| `attempt_count` | `Integer` | Attempts (default=`0`; readonly=`True`) |
| `last_attempt_at` | `Datetime` | Last Attempt (readonly=`True`) |
| `next_attempt_at` | `Datetime` | Next Attempt (index=`True`; readonly=`True`) |
| `state` | `Selection` | no label or help in the source (values `pending`, `done`, `abandoned`; required=`True`; default=`"pending"`; index=`True`; readonly=`True`) |

**Class attributes.** `_name = "cloudflare.hostname.pending.delete"`; `_description = "Cloudflare Custom Hostname Pending Delete"`; `_order = "state, next_attempt_at, id"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `_next_attempt_at`, `_record_failed_delete`, `_attempt_delete`, `_attempt_one_delete`, `_fail_attempt`, `_cron_retry_pending_deletes`, `action_retry_now`, `action_mark_abandoned`, `_check_pending_delete_admin`.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflareIPBan` [@ANCHOR: cloudflare:cloudflare_ip_ban_cloudflare_ipban]

**Purpose and location.** Model description: Cloudflare IP Ban / Honeypot Registry. Defined in `cloudflare/models/ip_ban.py:9` as an Odoo model; bases: `models.Model`. (model `cloudflare.ip.ban`; default order "create_date desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `ip_address` | `Char` | Target IP Address (required=`True`) |
| `mode` | `Selection` | Action Applied (values `block`, `challenge`, `managed_challenge`; required=`True`; default=`"block"`) |
| `notes` | `Char` | Trigger Reason (default=`"Honeypot Triggered"`) |
| `cf_rule_id` | `Char` | Cloudflare Rule ID (readonly=`True`) |
| `state` | `Selection` | Status (values `active`, `lifted`, `failed`; default=`"active"`) |
| `website_id` | `Many2one` | Website (comodel `website`; default=`lambda self: self.env["website"].get_current_websi`) |

**Class attributes.** `_name = "cloudflare.ip.ban"`; `_description = "Cloudflare IP Ban / Honeypot Registry"`; `_order = "create_date desc"`; `_ip_website_uniq = models.Constraint(
        "UNIQUE(ip_address, website_id)",
        "An IP address can only be bann`; `_ip_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(ip_address)) > 0)", "The IP address cannot be empty."
`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_execute_ban`, `action_lift_ban`, `_action_lift_ban_sync`.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflarePurgeMixin` [@ANCHOR: cloudflare:cloudflare_purge_mixin_cloudflare_purge_mixin]

**Purpose and location.** Model description: Cloudflare Purge Mixin. Defined in `cloudflare/models/purge_mixin.py:6` as an Odoo model; bases: `models.AbstractModel`. (model `cloudflare.purge.mixin`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name |

**Class attributes.** `_name = "cloudflare.purge.mixin"`; `_description = "Cloudflare Purge Mixin"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_enqueue_cloudflare_purge`, `_purge_cloudflare_menus`.

**See also:** test `test_falsy_urls_in_the_list_are_skipped (tests/test_enqueue_urls_batch.py)`.

### `CloudflarePurgeQueue` [@ANCHOR: cloudflare:cloudflare_purge_queue_cloudflare_purge_queue]

**Purpose and location.** Model description: Cloudflare Cache Purge Queue. Defined in `cloudflare/models/purge_queue.py:17` as an Odoo model; bases: `models.Model`. (model `cloudflare.purge.queue`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `target_item` | `Char` | Target Payload (required=`False`) |
| `purge_type` | `Selection` | no label or help in the source (values `url`, `tag`, `everything`; required=`True`; default=`"url"`) |
| `state` | `Selection` | no label or help in the source (values `pending`, `failed`; default=`"pending"`; index=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; ondelete=`"cascade"`) |

**Class attributes.** `_name = "cloudflare.purge.queue"`; `_description = "Cloudflare Cache Purge Queue"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `enqueue_urls_batch`, `enqueue_urls`, `enqueue_tags`, `enqueue_everything`, `_pager_incidents`, `_pager_default_env`, `_report_purge_failure`, `_resolve_purge_failure`, `requeue_failed`, `process_queue`.

**See also:** test `test_blog_post_write_enqueues_a_purge (tests/test_bridge_and_misc.py)`.

### `CloudflarePurgeWizard` [@ANCHOR: cloudflare:cloudflare_purge_wizard_cloudflare_purge_wizard]

**Purpose and location.** Model description: Cloudflare Manual Cache Purge Wizard. Defined in `cloudflare/models/purge_wizard.py:9` as an Odoo model; bases: `models.TransientModel`. (model `cloudflare.purge.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `website_id` | `Many2one` | Website (comodel `website`; required=`True`; default=`lambda self: self.env["website"].get_current_websi`) |
| `purge_type` | `Selection` | Purge Type (values `everything`, `urls`, `tags`; required=`True`; default=`"everything"`) |
| `items_to_purge` | `Text` | Items to Purge: Enter URLs or Cache-Tags separated by a new line or comma. |

**Class attributes.** `_name = "cloudflare.purge.wizard"`; `_description = "Cloudflare Manual Cache Purge Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_purge`.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `ResConfigSettings` [@ANCHOR: cloudflare:cloudflare_res_config_settings_res_config_settings]

**Purpose and location.** Extends the Odoo model "res.config.settings" with the fields `cloudflare_api_token`, `cloudflare_zone_id`, `cloudflare_account_id`, `cloudflare_turnstile_secret`, `cloudflare_trust_non_tunnel_peers`, `cloudflare_trusted_ip_ranges_custom`, `cloudflare_trusted_ip_ranges_auto`, `cloudflare_trusted_ip_ranges_last_refreshed` and the methods `_compute_cloudflare_trusted_ip_ranges_display`, `_compute_cloudflare_secrets`, `_inverse_cloudflare_secrets`, `get_values`, `set_values`, `action_refresh_cloudflare_trusted_ip_ranges`. Defined in `cloudflare/models/res_config_settings.py:18` as an Odoo model; bases: `models.TransientModel`. (extends `res.config.settings`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `cloudflare_api_token` | `Char` | no label or help in the source (compute=`"_compute_cloudflare_secrets"`) |
| `cloudflare_zone_id` | `Char` | mirrors the related field `"website_id.cloudflare_zone_id"` (related=`"website_id.cloudflare_zone_id"`; readonly=`False`) |
| `cloudflare_account_id` | `Char` | mirrors the related field `"website_id.cloudflare_account_id"` (related=`"website_id.cloudflare_account_id"`; readonly=`False`) |
| `cloudflare_turnstile_secret` | `Char` | no label or help in the source (compute=`"_compute_cloudflare_secrets"`) |
| `cloudflare_trust_non_tunnel_peers` | `Boolean` | Trust Cloudflare's Published IP Ranges: Only turn this on if Cloudflare connects to this origin directly over the network instead of through a Cloudflare Tunnel. Leave it off for a Tunnel-only deployment: cloudflared's connection is loopback and is already trusted unconditionally. Turning this on also needs authenticated origin pulls (Cloudflare's mTLS feature) or a per-zone firewall rule restricting the origin to Cloudflare's edge -- set up separately -- or it only adds trust without adding real protection, since Cloudflare's ranges front every one of its customers, not just this zone. |
| `cloudflare_trusted_ip_ranges_custom` | `Char` | Additional Trusted IP Ranges |
| `cloudflare_trusted_ip_ranges_auto` | `Char` | Auto-Fetched Cloudflare IP Ranges (compute=`"_compute_cloudflare_trusted_ip_ranges_display"`) |
| `cloudflare_trusted_ip_ranges_last_refreshed` | `Char` | Last Refreshed (compute=`"_compute_cloudflare_trusted_ip_ranges_display"`) |

**Class attributes.** `_inherit = "res.config.settings"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_cloudflare_trusted_ip_ranges_display`, `_compute_cloudflare_secrets`, `_inverse_cloudflare_secrets`, `get_values`, `set_values`, `action_refresh_cloudflare_trusted_ip_ranges`, `action_deploy_cf_waf`, `action_pull_cf_waf`, `action_generate_tunnel_command`.

**See also:** test `test_trigger_edge_purge_static_assets_enqueues_on_mtime_increase (tests/test_bridge_and_misc.py)`.

### `CloudflareTenantKey` [@ANCHOR: cloudflare:cloudflare_tenant_key_cloudflare_tenant_key]

**Purpose and location.** Model description: Per-Company Cloudflare Encryption Key (Envelope-Encrypted). Defined in `cloudflare/models/tenant_key.py:13` as an Odoo model; bases: `models.Model`. (model `cloudflare.tenant.key`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | no label or help in the source (compute=`"_compute_name"`; store=`True`) |
| `company_id` | `Many2one` | no label or help in the source (comodel `res.company`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `wrapped_key` | `Char` | Wrapped Per-Company Key (required=`True`) |

**Class attributes.** `_name = "cloudflare.tenant.key"`; `_description = "Per-Company Cloudflare Encryption Key (Envelope-Encrypted)"`; `_company_uniq = models.Constraint(
        "unique(company_id)",
        "Only one Cloudflare tenant key is allowed `.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `_get_or_create_fernet`.

**See also:** test `test_two_companies_get_independent_keys (tests/test_tenant_key.py)`.

### `CloudflareTrustedIpUtils` [@ANCHOR: cloudflare:cloudflare_trusted_ip_ranges_cloudflare_trusted_ip_utils]

**Purpose and location.** Model description: Cloudflare Trusted IP Range Utilities. Defined in `cloudflare/models/trusted_ip_ranges.py:136` as an Odoo model; bases: `models.AbstractModel`. (model `cloudflare.trusted_ip_utils`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "cloudflare.trusted_ip_utils"`; `_description = "Cloudflare Trusted IP Range Utilities"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_effective_trusted_ip_ranges`, `_is_trusted_cf_peer`, `_publish_trusted_ip_ranges_to_redis`, `_cron_refresh_cloudflare_ip_ranges`.

**See also:** test `test_01c_get_request_context_from_an_allowlisted_non_tunnel_peer_trusts_cf_headers (tests/test_request_context.py)`.

### `CloudflareTunnel` [@ANCHOR: cloudflare:cloudflare_tunnel_cloudflare_tunnel]

**Purpose and location.** Model description: Cloudflare Tunnel. Defined in `cloudflare/models/tunnel.py:35` as an Odoo model; bases: `models.Model`. (model `cloudflare.tunnel`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `cf_tunnel_id` | `Char` | Tunnel ID (required=`True`; readonly=`True`) |
| `name` | `Char` | Tunnel Name (required=`True`; readonly=`True`) |
| `status` | `Char` | Status (readonly=`True`) |
| `created_at` | `Datetime` | Created At (readonly=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; default=`lambda self: self.env["website"].get_current_websi`; readonly=`True`) |
| `route_ids` | `One2many` | Routing Table (comodel `cloudflare.tunnel.route`) |
| `routes_provisioned` | `Boolean` | Routes Provisioned: Set once this tunnel's ingress configuration has been pushed to Cloudflare. Cleared value means the next daemon start will push it (and retry on every later start until it succeeds). (default=`False`; readonly=`True`) |
| `ssh_route_enabled` | `Boolean` | Publish SSH Route: SSH is NOT published unless this is set. When set, a push adds ssh.<website domain> -> ssh://localhost:22 just before the catch-all rule. It needs the website's domain to be set, and a DNS record for that ssh. name to be reachable. (default=`False`) |
| `catch_all_service` | `Char` | Catch-all Service: Service that answers every hostname no other rule matches; always the last rule of a push. The default is this Odoo (port 8069). An http(s) URL or http_status:<code> (for example http_status:404). Note: in production with an nginx buffering front end, point this to the reverse proxy (e.g. port 8085) to prevent slow clients from truncating large downloads. (required=`True`; default=`DEFAULT_CATCH_ALL_SERVICE`) |

**Class attributes.** `_name = "cloudflare.tunnel"`; `_description = "Cloudflare Tunnel"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_catch_all_service`, `_check_tunnel_caller_authorized`, `_build_ingress`, `_ingress_problems`, `action_push_configuration`, `action_delete_tunnel`, `action_sync_tunnels`, `_sync_tunnels_for_website`, `_migrate_global_provisioned_flag`, `_ensure_one_tunnel_running`, `action_ensure_tunnel_running`.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `CloudflareTunnelRoute` [@ANCHOR: cloudflare:cloudflare_tunnel_route_cloudflare_tunnel_route]

**Purpose and location.** Model description: Cloudflare Tunnel Route. Defined in `cloudflare/models/tunnel_route.py:10` as an Odoo model; bases: `models.Model`. (model `cloudflare.tunnel.route`; default order "sequence, id".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Route (compute=`"_compute_name"`; store=`True`) |
| `tunnel_id` | `Many2one` | Tunnel: If empty, this acts as a Global Route Template applied to all tunnels. (comodel `cloudflare.tunnel`; ondelete=`"cascade"`) |
| `sequence` | `Integer` | Sequence: Position in the pushed ingress list. Must be unique among this tunnel's routes and the global templates. (default=`10`) |
| `hostname` | `Char` | Hostname: e.g. api.hams.com (leave empty to match all) |
| `path` | `Char` | Path: Regular expression matched against the request path, e.g. ^/adif$ (anchor it with ^ and $ to match only that path; an unanchored /ws also matches /ws/daemon_uplink). Leave empty to match all. |
| `service_url` | `Char` | Service URL: e.g. http://localhost:8070, tcp://localhost:22, or http_status:404 (required=`True`) |

**Class attributes.** `_name = "cloudflare.tunnel.route"`; `_description = "Cloudflare Tunnel Route"`; `_order = "sequence, id"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `_check_hostname_is_not_a_member_name`.

**See also:** test `test_tunnel_route_compute_name_reflects_hostname_path_and_service (tests/test_bridge_and_misc.py)`.

### `CloudflareTunnelWizard` [@ANCHOR: cloudflare:cloudflare_tunnel_wizard_cloudflare_tunnel_wizard]

**Purpose and location.** Model description: Cloudflare Tunnel Setup Wizard. Defined in `cloudflare/models/tunnel_wizard.py:6` as an Odoo model; bases: `models.TransientModel`. (model `cloudflare.tunnel.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `command` | `Text` | Installation Command: Copy and execute this command in your terminal to install the Cloudflare Tunnel. (compute=`"_compute_command"`; readonly=`True`) |

**Class attributes.** `_name = "cloudflare.tunnel.wizard"`; `_description = "Cloudflare Tunnel Setup Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_command`.

**See also:** test `test_03_tunnel_setup (tests/test_cloudflare_apis.py)`.

### `CloudflareTurnstile` [@ANCHOR: cloudflare:cloudflare_turnstile_cloudflare_turnstile]

**Purpose and location.** Model description: Cloudflare Turnstile Interface. Defined in `cloudflare/models/turnstile.py:7` as an Odoo model; bases: `models.AbstractModel`. (model `cloudflare.turnstile`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |

**Class attributes.** `_name = "cloudflare.turnstile"`; `_description = "Cloudflare Turnstile Interface"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `verify_token`.

**See also:** test `test_02_turnstile_secret_fetch (tests/test_cloudflare_apis.py)`.

### `CloudflareWAF` [@ANCHOR: cloudflare:cloudflare_waf_cloudflare_waf]

**Purpose and location.** Model description: Cloudflare WAF Interface. Defined in `cloudflare/models/waf.py:7` as an Odoo model; bases: `models.AbstractModel`. (model `cloudflare.waf`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |

**Class attributes.** `_name = "cloudflare.waf"`; `_description = "Cloudflare WAF Interface"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `ban_ip`.

**See also:** test `test_01_ban_and_unban_use_the_access_rules_endpoint (tests/test_cloudflare_api_calls.py)`.

### `CloudflareWafRule` [@ANCHOR: cloudflare:cloudflare_waf_rule_cloudflare_waf_rule]

**Purpose and location.** Model description: Cloudflare WAF Custom Rule. Defined in `cloudflare/models/waf_rule.py:6` as an Odoo model; bases: `models.Model`. (model `cloudflare.waf.rule`; default order "sequence, id".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `sequence` | `Integer` | Sequence (default=`10`) |
| `name` | `Char` | Rule Name (required=`True`) |
| `action` | `Selection` | Action (values `block`, `challenge`, `managed_challenge`, `js_challenge`, `skip`; required=`True`; default=`"managed_challenge"`) |
| `expression` | `Text` | Expression: Cloudflare Ruleset Engine expression (e.g. (http.request.uri.path contains "/xmlrpc")). (required=`True`) |
| `description` | `Text` | Comments / Documentation |
| `active` | `Boolean` | Active (default=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; required=`True`; default=`lambda self: self.env["website"].get_current_websi`) |

**Class attributes.** `_name = "cloudflare.waf.rule"`; `_description = "Cloudflare WAF Custom Rule"`; `_order = "sequence, id"`; `_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "The rule name cannot be empty."
    )`; `_expr_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(expression)) > 0)", "The expression cannot be empty."
`; `_name_website_uniq = models.Constraint(
        "UNIQUE(name, website_id)",
        "A rule with this name already exists`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `WebsiteCloudflare` [@ANCHOR: cloudflare:cloudflare_website_website_cloudflare]

**Purpose and location.** Extends the Odoo model "website" with the fields `cloudflare_ip_ban_ids`, `cloudflare_tunnel_ids`, `cloudflare_waf_rule_ids`, `cloudflare_purge_queue_ids`, `cloudflare_api_token_crypt`, `cloudflare_api_token`, `cloudflare_zone_id`, `cloudflare_account_id` and the methods `write`, `_register_secrets_with_canary`, `_get_fernet`, `_crypt_field`, `_compute_encrypted_field`, `_inverse_encrypted_field`. Defined in `cloudflare/models/website.py:32` as an Odoo model; bases: `models.Model`. (extends `website`.)

| Field | Type | Meaning |
|---|---|---|
| `cloudflare_ip_ban_ids` | `One2many` | no label or help in the source (comodel `cloudflare.ip.ban`) |
| `cloudflare_tunnel_ids` | `One2many` | no label or help in the source (comodel `cloudflare.tunnel`) |
| `cloudflare_waf_rule_ids` | `One2many` | no label or help in the source (comodel `cloudflare.waf.rule`) |
| `cloudflare_purge_queue_ids` | `One2many` | no label or help in the source (comodel `cloudflare.purge.queue`) |
| `cloudflare_api_token_crypt` | `Char` | Encrypted CF API Token |
| `cloudflare_api_token` | `Char` | CF API Token: Required to authenticate with Cloudflare API. (compute=`"_compute_cf_api_token"`) |
| `cloudflare_zone_id` | `Char` | CF Zone ID: The Zone ID of your domain on Cloudflare. |
| `cloudflare_account_id` | `Char` | CF Account ID: The Account ID associated with your Cloudflare account. |
| `cloudflare_turnstile_secret_crypt` | `Char` | Encrypted Turnstile Secret |
| `cloudflare_turnstile_secret` | `Char` | Turnstile Secret: Secret key for Cloudflare Turnstile integration. (compute=`"_compute_cf_turnstile_secret"`) |

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `write`, `_register_secrets_with_canary`, `_get_fernet`, `_crypt_field`, `_compute_encrypted_field`, `_inverse_encrypted_field`, `_compute_cf_api_token`, `_inverse_cf_api_token`, `_compute_cf_turnstile_secret`, `_inverse_cf_turnstile_secret`, `_get_cloudflare_credentials`.

**See also:** test `test_trigger_edge_purge_static_assets_enqueues_on_mtime_increase (tests/test_bridge_and_misc.py)`.

### `CloudflareZoneSettingsWizard` [@ANCHOR: cloudflare:cloudflare_zone_settings_wizard_cloudflare_zone_settings_wizard]

**Purpose and location.** Model description: Cloudflare Zone Settings Wizard. Defined in `cloudflare/models/zone_settings_wizard.py:9` as an Odoo model; bases: `models.TransientModel`. (model `cloudflare.zone.settings.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `website_id` | `Many2one` | Website (comodel `website`; required=`True`; default=`lambda self: self.env["website"].get_current_websi`) |
| `security_level` | `Selection` | Security Level: Choose the Cloudflare security profile... (values `essentially_off`, `low`, `medium`, `high`, `under_attack`) |
| `development_mode` | `Selection` | Development Mode: Temporarily bypass Cloudflare cache... (values `on`, `off`) |
| `always_use_https` | `Selection` | Always Use HTTPS: Redirect every plain-HTTP request for the zone to HTTPS at Cloudflare's edge, before it reaches the origin. Leave unset to keep the zone's current value. (values `on`, `off`) |
| `hsts_status` | `Selection` | HSTS (Strict-Transport-Security): Enable or disable HTTP Strict Transport Security (HSTS) at Cloudflare's edge. Leave unset to keep the zone's current value. (values `on`, `off`) |
| `hsts_max_age` | `Integer` | HSTS Max Age (seconds): Max age in seconds for Strict-Transport-Security header (e.g. 86400 for 1 day, 31536000 for 1 year). (default=`86400`) |
| `hsts_include_subdomains` | `Boolean` | HSTS Include Subdomains: Whether to include subdomains in the HSTS header. |
| `hsts_nosniff` | `Boolean` | HSTS No-Sniff: Whether to include the X-Content-Type-Options: nosniff header. (default=`True`) |
| `hsts_preload` | `Boolean` | HSTS Preload: Permit inclusion of this domain in the HSTS preload list. |
| `browser_cache_ttl` | `Integer` | Browser Cache TTL (seconds): Time in seconds. 0 means respect existing headers. |
| `browser_cache_respect_headers` | `Boolean` | Respect Existing Headers: Send Browser Cache TTL 0, so the browser cache time is the origin's own Cache-Control, not a zone value. |

**Class attributes.** `_name = "cloudflare.zone.settings.wizard"`; `_description = "Cloudflare Zone Settings Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `default_get`, `action_apply_settings`.

**See also:** test `test_website_menu_write_and_unlink_purge_all_menus (tests/test_bridge_and_misc.py)`.

### `IdempotencyAwareRetry` [@ANCHOR: cloudflare:cloudflare_cloudflare_api_idempotency_aware_retry]

**Purpose and location.** Retry 5xx only for idempotent methods; retry POST/PATCH only on 429.  urllib3 decides retries from one ``status_forcelist`` shared by every method, gated by ``allowed_methods``. That is too coarse here: we want the full transient-failure list for GET and friends, and only a rate-limit refusal for POST and PATCH. Overriding ``is_retry`` is what splits the two, and ``Retry.new()`` constructs ``type(self)``, so the subclass survives every retry in a sequence rather than degrading to the base class.  ``allowed_methods`` deliberately stays restricted to the idempotent set so that urllib3's own read-error handling keeps refusing to replay a POST whose response was lost in transit -- that request may well have been applied. A connection error is exempt in urllib3 regardless of method, correctly: the request was never delivered. Defined in `cloudflare/utils/cloudflare_api.py:43` as a class; bases: `Retry`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `is_retry`.

**See also:** test `test_08_retry_policy_excludes_non_idempotent_methods (tests/test_cloudflare_apis.py)`.
