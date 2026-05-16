# 📚 Manual Library Module (`manual_library`)

*Copyright © Bruce Perens K6BP. AGPL-3.0.*

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators.
</system_role>

<architecture>
## 1. Architecture
A clean-room, 100% drop-in API replacement for the proprietary Odoo Enterprise Knowledge module (`knowledge.article`).

## 2. Interoperability
* Dependent modules inject documentation using standard XML records targeting `model="knowledge.article"`.
* **Fields Supported:** `name`, `body` (HTML), `parent_id`, `sequence`, `is_published`, `icon`, `active`, `internal_permission`.
* If the system is upgraded to Enterprise, the table structure allows perfect data retention.
</architecture>

<features>
## 3. Core Features & Logic
* **User Feedback:** Handles user submissions of helpful/not-helpful article ratings via the feedback controller `[@ANCHOR: controller_manual_feedback]`.
* **Search Integration:** Supports live querying of article contents via the search controller `[@ANCHOR: controller_manual_search]`.
* **URL Resolution:** Computes the public website URL path for articles dynamically based on their hierarchy `[@ANCHOR: manual_compute_website_url]`.
* **Structural Integrity:** Strictly enforces parent-child hierarchy checks to prevent recursive or invalid tree structures `[@ANCHOR: manual_check_hierarchy]`.
* **Dynamic TOC:** Automatically parses article HTML on the frontend to generate a dynamic Table of Contents `[@ANCHOR: manual_toc_logic]`.
  * **Automated Documentation Installation:** Implements a global `_register_hook` on `ir.module.module` to automatically discover and install documentation from `data/documentation.html` or `README.md` for all installed modules once the registry is ready, supporting soft dependencies on `knowledge.article` or `manual.article` `[@ANCHOR: manual_doc_auto_install]`.
</features>

---

<stories_and_journeys>
## 4. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Article Feedback](docs/stories/manual_library/feedback.md) `[@ANCHOR: story_manual_feedback]`
* [Article Hierarchy Integrity](docs/stories/manual_library/hierarchy.md) `[@ANCHOR: story_manual_hierarchy]`
* [Automated Documentation Installation](docs/stories/manual_library/doc_installation.md) `[@ANCHOR: story_manual_doc_installation]`
* [Backend Management Views](docs/stories/manual_library/backend_views.md) `[@ANCHOR: story_manual_backend_views]`
* [Dynamic Table of Contents](docs/stories/manual_library/toc.md) `[@ANCHOR: story_manual_toc]`
* [Dynamic URL Generation](docs/stories/manual_library/url_generation.md) `[@ANCHOR: story_manual_url_generation]`
* [Searching the Manual](docs/stories/manual_library/search.md) `[@ANCHOR: story_manual_search]`
* [Viewing Manual Articles](docs/stories/manual_library/article_view.md) `[@ANCHOR: story_article_view]`

### Journeys
* [Administrator Managing Articles](docs/journeys/manual_library/admin_managing_articles.md) `[@ANCHOR: journey_admin_managing]`
* [Developer Integrating Documentation](docs/journeys/manual_library/developer_doc_integration.md) `[@ANCHOR: journey_developer_integration]`
* [User Browsing the Manual](docs/journeys/manual_library/user_browsing_journey.md) `[@ANCHOR: journey_user_browsing]`
</stories_and_journeys>
