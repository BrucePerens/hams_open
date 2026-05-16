# Manual Library (`manual_library`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

This is a free, open-source replacement for Odoo's Enterprise Knowledge app. It lets you write, organize, and publish documentation and user manuals directly inside Odoo Community.

Because it uses the exact same database structure (`knowledge.article`) as the Enterprise version, other modules can easily install their own instruction manuals here without breaking.

**Open Source Rule:** We built this for the open-source community. It runs perfectly on its own and does not rely on any proprietary code.

## 🌟 Key Features

* **Nested Folders:** Organize your articles in a tree (parent/child) so they are easy to navigate.
* **Enterprise Compatible:** You can load XML data files meant for the Enterprise Knowledge app, and they will work perfectly here.
* **Rich Text Editor:** Use Odoo's standard editor to write guides, insert images, and format text.
* **Public Web Portal:** Click "Publish" to instantly push your manuals to the public website (`/manual`). The system automatically builds a handy sidebar menu for visitors.
* **Access Control:** Keep private admin notes hidden, share drafts with logged-in coworkers, or publish finalized guides to the public.

## 🛠️ Installation

1. Drop the `manual_library` folder into your Odoo `addons` directory.
2. Restart your Odoo server.
3. Turn on Developer Mode, go to **Apps**, and click **Update Apps List**.
4. Search for `Manual Library` and click **Install**.

## 📖 How to Use It

### Writing Articles
1. Click the **Manuals** app in the main Odoo menu.
2. Click **New**.
3. If you want this article to sit inside a folder, pick a **Parent Article**.
4. When you're ready to share it with the world, hit the **Is Published** button at the top.

### Reading Articles on the Web
* Go to `/manual` on your website to see the public knowledge base.
* We included a search bar and an automatically generated Table of Contents that reads your headers so users can jump around long documents easily.

## ⚖️ Legal Note

We built this from scratch. We did not copy any code, logic, or proprietary designs from Odoo Enterprise. We just matched the database field names so the two systems are perfectly compatible (known legally as API Interoperability).

---

# Technical Documentation

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
  * **Automated Documentation Installation:** Utilizes the central `_bootstrap_knowledge_docs` facility from the `zero_sudo` module to automatically discover and install documentation for all installed modules via the `knowledge_docs` manifest key. This supports soft dependencies on `knowledge.article` or `manual.article` `[@ANCHOR: manual_doc_auto_install]`.
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
