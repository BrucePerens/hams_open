# Global Compliance & Privacy (`compliance`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

This module helps your website stay legal. It handles the technical requirements of privacy laws like GDPR and CCPA automatically, so you don't have to worry about the details.

## 🌟 What It Does

* **Automatic Cookie Banner:** It turns on the standard Odoo cookie banner on all your websites. This banner asks visitors for permission before any optional tracking starts. New websites you create will also have this turned on automatically.
* **Ready-to-Use Legal Pages:** It creates basic versions of the three pages every website needs: a Privacy Policy, a Cookie Policy, and Terms of Service.
* **Protects Your Changes:** If you edit these pages using the website builder, your changes are safe. We won't overwrite them when you update the module. If you already had your own legal pages, we'll keep yours and hide ours.

## ⚖️ What's in the Policies?
The policies we provide are designed to work with our other modules. They explain:
* How we count visitors without tracking them.
* How users can see or delete their data from their account dashboard.
* How we protect the identity of people who report problems or abuse.
* How our simple "three-strikes" system keeps the community safe.

## 📖 How to Manage Your Website's Compliance

### 1. Editing Your Legal Pages
You can find your legal pages at these addresses on your site:
* **Privacy Policy:** `/privacy`
* **Cookie Policy:** `/cookie-policy`
* **Terms of Service:** `/terms`

**To change the text on these pages:**
1. Go to the page on your website.
2. Click **Edit** at the top right.
3. Type your changes directly into the page.
4. Click **Save**.

### 2. Checking the Cookie Banner
The cookie banner shows up for new visitors. To see it yourself:
1. Open your browser in "Private" or "Incognito" mode.
2. Go to your website.
3. You should see a banner at the bottom of the screen.

### 3. Using Your Own Custom Pages
If you already made your own page at `/privacy` (or the other addresses) before installing this module, we won't change it. We'll hide our version so visitors only see yours.
* **To use our version instead:** Just delete or rename your custom page, and our standard version will show up automatically.

## 🧪 Testing

To run the tests for this module in the Jules VM environment:

```bash
IN_JULES_VM=1 python3 tools/test.py -u compliance --already-provisioned
```

## 🛠️ Installation

1. Drop the `compliance` folder into your Odoo `addons` directory.
2. Restart your Odoo server.
3. Turn on Developer Mode, go to **Apps**, and click **Update Apps List**.
4. Search for `Global Compliance` and click **Install**.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators.
</system_role>

<enforcement_details>
## 1. Overview
A non-interactive configuration module that enforces baseline regulatory compliance across the Odoo instance upon installation.

### 📚 User Stories & Journeys

#### Stories
* [Automatic Legal Pages Generation](./docs/stories/automatic_legal_pages.md) `[@ANCHOR: story_automatic_legal_pages]`
* [Enforced Cookie Consent](./docs/stories/cookie_consent.md) `[@ANCHOR: story_cookie_consent]`
* [Site Owner Documentation](./docs/stories/compliance_documentation.md) `[@ANCHOR: story_compliance_documentation]`

#### Journeys
* [Compliance Setup Journey](./docs/journeys/compliance_setup_journey.md) `[@ANCHOR: journey_compliance_setup]`

## 2. Enforcement Details
* **Automated Cookie Consent:** Programmatically enables the Odoo `website` native `cookies_bar` boolean on install and sets it as the default for new websites. `[@ANCHOR: compliance_post_init_cookie_bar]`
* **Safe Legal Page Provisioning:** Provisions AGPL-3 compatible legal pages safely via `noupdate="1"` XML records. `[@ANCHOR: compliance_legal_pages_rendering]`
    * Privacy Policy Template `[@ANCHOR: compliance_privacy_policy_template]`
    * Cookie Policy Template `[@ANCHOR: compliance_cookie_policy_template]`
    * Terms of Service Template `[@ANCHOR: compliance_terms_of_service_template]`
* **Non-Destructive Mandate:** If a page already exists at one of the target URLs, the module's boilerplate is unpublished to avoid duplication. `[@ANCHOR: test_compliance_non_destructive_mandate]`
* **Editability Mandate:** Legal pages are standard `website.page` records, allowing administrators to use the Odoo website builder for customization.

## 3. API & Integration
### Standardized Routes
Dependent modules requiring legal links MUST use:
* `/privacy` : Privacy Policy
* `/cookie-policy` : Cookie Policy
* `/terms` : Terms of Service

### Integration Rules
1. **Do Not Build Custom Banners:** Rely entirely on Odoo's native `website.cookies_bar`.
2. **Tracking Scripts:** Any third-party JavaScript tracking MUST hook into the Odoo consent state.
</enforcement_details>

<security_architecture>
## 4. Security & Zero-Sudo
This module adheres to **ADR-0002 (Zero-Sudo)** and **ADR-0005 (Service Account Web Isolation)**.

* **Micro-Privilege Account:** Automated post-install configuration is executed via the `compliance.user_compliance_service` service account.
* **ACLs:** The service account is granted minimal read/write access to `website`, `website.page`, and `ir.ui.view` models. `[@ANCHOR: compliance_security_acls]`
* **Impersonation:** Escalation is handled via `env.with_user(svc_uid)` instead of `.sudo()` for core operations. `[@ANCHOR: compliance_zero_sudo_impersonation]`

## 5. Website-Aware Scope
The module is multi-website aware. When detecting custom pages at target URLs, it only unpublishes the boilerplate for the specific website scope (or global scope) where the custom page is found. If a custom page is removed, the boilerplate is automatically restored. `[@ANCHOR: compliance_website_aware_scope]`

## 6. Documentation Installation
This module implements a **soft dependency** on documentation providers (`manual_library` or Odoo Enterprise `knowledge`).

* **Mechanism:** Documentation is automatically provisioned during the final registry reload by the central engine (`_bootstrap_knowledge_docs` in `zero_sudo`). `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]`
* **Article Title:** "Site Owner's Guide to Regulatory Compliance"

## 7. Verification and Testing
Comprehensive test coverage ensures ongoing compliance:
* **Hook Testing:** `test_hooks.py` verifies `cookies_bar` enforcement and non-destructive page provisioning.
* **Page Integrity:** `test_pages.py` ensures all legal routes are active and contain valid boilerplate content.
* **Security Audit:** `test_security.py` confirms service account configuration and hook idempotency.
* **UI Tours:** `compliance_tour.js` simulates end-to-end user navigation across all legal pages. `[@ANCHOR: test_compliance_ui_tour]`
</security_architecture>
