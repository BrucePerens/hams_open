# Manual Library (`manual_library`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

The **Manual Library** is a powerful, free, and open-source replacement for Odoo's Enterprise Knowledge app. It provides a complete documentation system that allows you to write, organize, and publish user manuals, standard operating procedures (SOPs), and help guides directly inside Odoo Community.

This module was designed with **API Interoperability** in mind. It uses the exact same database structure (`knowledge.article`) as the Enterprise version, meaning other modules can install their own instruction manuals here without any modifications, and you can upgrade to Enterprise later without losing your data.

## 🌟 Key Features

*   **Hierarchical Organization:** Organize your articles into a tree structure with parent and child articles, making it easy to navigate complex documentation.
*   **Enterprise Compatibility:** Fully supports XML data files meant for the Enterprise Knowledge app.
*   **Powerful Rich Text Editor:** Leverage Odoo's native editor to create beautiful guides with images, tables, and advanced formatting.
*   **Instant Public Publishing:** One-click publishing pushes your manuals to the public website at `/manual`. The system automatically builds a recursive sidebar and breadcrumbs for visitors.
*   **Granular Access Control:** Keep your internal notes private, collaborate with team members on drafts, or share guides with customers via the portal.
*   **Dynamic Table of Contents (TOC):** Automatically generates a sticky TOC on the frontend based on your article's headings (H2 and H3).
*   **Multi-Tenant Isolation:** Strictly respects Odoo's multi-website and multi-company architectures.
*   **Zero-Sudo Security:** Built on a secure "Zero-Sudo" foundation, using micro-privilege service accounts for automated tasks.

## 🛠️ Installation

1.  Place the `manual_library` folder into your Odoo `addons` directory.
2.  Restart your Odoo service.
3.  In Odoo, activate **Developer Mode**, navigate to **Apps**, and click **Update Apps List**.
4.  Search for **Manual Library** and click **Install**.

## 📖 How to Use It

### Creating and Managing Articles
1.  Open the **Manuals** app from the main Odoo menu.
2.  Click **New** to create a new article.
3.  **Hierarchy:** Use the **Parent Article** field to nest your article inside a folder or under another guide.
4.  **Permissions:** Set the **Internal Permission** to control who can see it:
    *   **Read Only:** Visible to all staff and portal users.
    *   **Read & Write:** Collaborative mode where all staff can edit.
    *   **No Access:** Private—only visible to you and people you explicitly add in the **Shared Members** tab.
5.  **Multi-Tenant:** Select a specific **Website** or **Company** to limit the article's visibility to that context.
6.  **Publishing:** When ready, click the **Is Published** button in the top right to make the guide visible to the general public at `/manual`.

### Reading on the Web
*   Navigate to `/manual` on your website.
*   Use the **Sidebar** to browse the hierarchy.
*   Use the **Search Bar** to find specific topics across all accessible articles.
*   Use the **Table of Contents** on the right side of articles to jump between sections.

## ⚖️ Legal Note

This module is a clean-room implementation. No code, logic, or proprietary designs from Odoo Enterprise were used. We have matched the database schema solely for API Interoperability and data portability.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation for LLMs, Developers, and Integrators.
This module provides a hierarchical documentation system. It is designed to be fully compatible with the `knowledge.article` model from Odoo Enterprise.
</system_role>

<architecture>
## 1. Architecture
A clean-room, 100% drop-in API replacement for the proprietary Odoo Enterprise Knowledge module (`knowledge.article`).
Inherits from `mail.thread`, `mail.activity.mixin`, `website.published.mixin`, and `website.multi.mixin`.

## 2. Interoperability
*   **Fields Supported:** `name`, `body` (HTML), `parent_id`, `sequence`, `is_published`, `icon`, `active`, `internal_permission`, `member_ids`.
*   Supports automated doc installation via the `knowledge_docs` manifest key and the `zero_sudo` bootstrap facility.
</architecture>

<features>
## 3. Core Features & Logic
*   **Article Feedback:** Atomic helpfulness increments via raw SQL and service accounts `[@ANCHOR: controller_manual_feedback]`.
*   **Search Engine:** Full-text search with multi-tenant filtering `[@ANCHOR: controller_manual_search]`.
*   **URL Resolution:** Dynamic slug generation including ID prefix `[@ANCHOR: manual_compute_website_url]`.
*   **Hierarchy Integrity:** Recursive cycle detection using `_has_cycle()` `[@ANCHOR: manual_check_hierarchy]`.
*   **Recursive Breadcrumbs:** Path computation from root to current node `[@ANCHOR: manual_compute_breadcrumbs]`.
*   **Reading Time Calculation:** Automatic estimation of reading time based on word count `[@ANCHOR: manual_compute_reading_time]`.
*   **Author Attribution:** Automatically identifies and displays the article author based on the last editor.
</features>

<security>
## 4. Security and Access Rights
*   **Public:** `is_published=True` and `website_id` matches.
*   **Portal/Internal:** `internal_permission != 'none'`, or `is_published=True`, or member of `member_ids`, or `create_uid`.
*   **Admin:** Full CRUD.
*   **Service Account:** `manual_library.user_manual_library_service_account` for background operations.
</security>
