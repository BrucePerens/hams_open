# Data structures

### `ManualLibraryController` [@ANCHOR: knowledge:knowledge_main_manual_library_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `_compile_markdown`, `_is_manual_manager`, `_help_domain`, `_internal_domain`, `_help_topics`, `_can_open_article`, `_get_sidebar_articles`, `_sidebar_context`. Defined in `knowledge/controllers/main.py:33` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compile_markdown`, `_is_manual_manager`, `_help_domain`, `_internal_domain`, `_help_topics`, `_can_open_article`, `_get_sidebar_articles`, `_sidebar_context`, `manual_article_view`, `_render_help_index`, `manual_internal_index`, `manual_search`, `knowledge_home_alias`, `manual_article_by_name`, `manual_feedback`.

**See also:** test `test_12_a_lower_sequence_leads_its_help_heading (tests/test_help_audience.py)`.

### `KnowledgeArticle` [@ANCHOR: knowledge:knowledge_knowledge_article_knowledge_article]

**Purpose and location.** Clean-room implementation of the knowledge.article API for Odoo Community. Defined in `knowledge/models/knowledge_article.py:56` as an Odoo model; bases: `models.Model`. (model `knowledge.article`; inherits [
        "mail.thread",
        "mail.activity.mixin",
        "website.published.mixin",
        "; default order "sequence, id".)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Title (required=`True`; index=`"trigram"`) |
| `body` | `Html` | Body |
| `parent_id` | `Many2one` | Parent Article (comodel `knowledge.article`; index=`True`; ondelete=`"restrict"`) |
| `child_ids` | `One2many` | Child Articles (comodel `knowledge.article`) |
| `sequence` | `Integer` | Sequence: Order of presentation. (default=`10`) |
| `is_published` | `Boolean` | Is Published (default=`False`) |
| `icon` | `Char` | Article Icon: Emoji or icon class for UI representation. |
| `active` | `Boolean` | Active (default=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; ondelete=`"cascade"`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; default=`lambda self: self.env.company`; ondelete=`"cascade"`) |
| `internal_permission` | `Selection` | Internal Permission (values `read`, `write`, `none`; default=`"read"`) |
| `category` | `Selection` | Category (values `workspace`, `private`, `shared`; default=`"workspace"`) |
| `audience` | `Selection` | Audience (values `member`, `internal`; required=`True`; default=`"member"`; index=`True`) |
| `help_topic` | `Char` | Help Topic: Heading the Help index files this member manual under. |
| `member_ids` | `Many2many` | Shared Members (comodel `res.users`) |
| `helpful_count` | `Integer` | Helpful Votes (default=`0`; readonly=`True`) |
| `unhelpful_count` | `Integer` | Unhelpful Votes (default=`0`; readonly=`True`) |
| `breadcrumb_article_ids` | `Many2many` | Breadcrumbs (comodel `knowledge.article`; compute=`"_compute_breadcrumb_article_ids"`) |
| `body_snippet` | `Text` | Body Snippet (compute=`"_compute_body_snippet"`) |
| `reading_time` | `Integer` | Reading Time (min) (compute=`"_compute_reading_time"`) |
| `author_id` | `Many2one` | Author (comodel `res.users`; compute=`"_compute_author_id"`; store=`True`) |

**Class attributes.** `_name = "knowledge.article"`; `_description = "Manual Article"`; `_inherit = [
        "mail.thread",
        "mail.activity.mixin",
        "website.published.mixin",
        "`; `_order = "sequence, id"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_author_id`, `_compute_breadcrumb_article_ids`, `_compute_body_snippet`, `_compute_reading_time`, `_check_hierarchy`, `copy`, `_compute_website_url`, `_search_get_detail`.

**See also:** test `test_02_explicit_opt_in_is_member (tests/test_doc_audience_installer.py)`.

### `Website` [@ANCHOR: knowledge:knowledge_website_website]

**Purpose and location.** Extends the Odoo model "website" with no new fields and the methods `_search_get_details`. Defined in `knowledge/models/website.py:7` as an Odoo model; bases: `models.Model`. (extends `website`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_search_get_details`.

**See also:** test `test_01_search_finds_a_published_article_by_title (tests/test_website_search.py)`.
