# Global Compliance & Privacy (`compliance`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

This module automatically handles the annoying parts of running a legal website. It makes sure your Odoo instance complies with GDPR (the EU General Data Protection Regulation), CCPA (the California Consumer Privacy Act), and ePrivacy (the EU ePrivacy Directive, often called the "Cookie Law") rules without you having to configure anything manually.

## 🌟 What It Does

* **Turns on the Cookie Banner:** As soon as you install this, it flips the switch to turn on Odoo's native Cookie Consent Bar across all your websites. It also ensures that any **new** websites created later have this enabled by default. This stops optional tracking scripts until the user clicks "Accept."
* **Writes Your Legal Pages:** It automatically creates standard, editable pages for your Privacy Policy (`/privacy`), Cookie Policy (`/cookie-policy`), Terms of Service (`/terms`), and Accessibility Statement (`/accessibility`).
* **Automatic Footer Links:** It injects links to all legal pages into your website's footer automatically, ensuring global visibility and regulatory compliance.
* **Privacy Dashboard Link:** Adds a direct link to the Privacy & Data Management dashboard (`/my/privacy`) in the footer, making it easy for users to exercise their data rights.
* **Compliance Documents Portal:** Automatically provides a centralized index of all active compliance documents at `/compliance` for easy user access.
* **Doesn't Break Your Edits:** If you've already written a privacy policy at `/privacy` when you install the module, the module detects it and leaves yours alone. If you edit the pages it creates, it won't overwrite your work when you update the module.

## ⚖️ Included Policy Coverage
The boilerplate policies we generate are written specifically to cover the features in our other open-source modules. They explain:
* How our privacy-friendly view counters work.
* How users can download or permanently delete their data at the `/my/privacy` dashboard.
* How our abuse reporting system hides the reporter's email to protect them.
* How our 3-strike moderation and suspension system works. (A strike is an upheld content violation; see [Content Moderation](../content_moderation/README.md) and the `user_websites` [README](../user_websites/README.md).)

## 📖 User Guide: Operating Your Compliant Website

Managing regulatory compliance is easy with this module. Here is how you can operate the core features:

### 1. Viewing and Editing Your Legal Pages
As a site owner, it is your responsibility to ensure that your legal documents accurately reflect your specific business practices and jurisdictional requirements. We provide standard templates that cover the default functionality of our modules, but you should review and customize them.

We provide four standard pages that you can edit to fit your needs:
* **Privacy Policy:** `/privacy`
* **Cookie Policy:** `/cookie-policy`
* **Terms of Service:** `/terms`
* **Accessibility Statement:** `/accessibility`

These pages, along with a link to the **My Privacy** dashboard (`/my/privacy`), are automatically linked in your website's footer for global visibility.

**To edit these pages:**
1. Navigate to the page URL on your website.
2. Click the **Edit** button in the top-right corner of the screen.
3. Use the Odoo website builder to change the text.
4. Click **Save**. Your changes are safe and won't be overwritten when you update the module.

### 2. The Cookie Consent Bar
The module automatically enables Odoo's native Cookie Bar. This ensures that no non-essential cookies are placed on a visitor's device until they have given their explicit consent.

* **To verify it:** Open your site in an "Incognito" or "Private" browser window (which ensures you are treated as a new visitor). You should see a banner at the bottom of the page.
* **To manage it:** You can find the settings under **Website -> Configuration -> Settings**. Search for "Cookie Bar". Here you can also customize the text of the banner and the "Learn More" link if you wish to point it to a specific page other than our default `/cookie-policy`.

### 3. Handling Your Own Legal Pages
If you already had a page at `/privacy` before installing this module, we won't touch it. Our "boilerplate" page will stay hidden (unpublished) so your visitors only see your version.
* **To switch to our boilerplate:** Delete or rename your custom page, then re-publish our version (same page name, e.g. "Privacy Policy") in the **Site -> Pages** menu. It does not become visible again on its own: the check that hides and restores the boilerplate runs only when the module is installed (or reinstalled), not on a module update or when you delete a page.

## 🧪 Testing

To run the tests for this module:

```bash
sudo -u odoo env HAMS_ISOLATED_NS=1 python3 hams_shared/tools/test.py -u compliance
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
A non-interactive configuration module that enforces baseline regulatory compliance across the Odoo instance upon installation. This module is essential for making the platform accessible and trustworthy for the general population.

### 📚 User Stories & Journeys

#### Stories
* [Automatic Legal Pages Generation](./docs/stories/automatic_legal_pages.md) `[@ANCHOR: COMM_story_automatic_legal_pages]`

* [Enforced Cookie Consent](./docs/stories/cookie_consent.md) `[@ANCHOR: COMM_story_cookie_consent]`

* [Site Owner Documentation](./docs/stories/compliance_documentation.md) `[@ANCHOR: COMM_story_compliance_documentation]`

#### Journeys
* [Compliance Setup Journey](./docs/journeys/compliance_setup_journey.md) `[@ANCHOR: COMM_journey_compliance_setup]`

## 2. Enforcement Details
* **Automated Cookie Consent:** Programmatically enables the Odoo `website` native `cookies_bar` boolean on install and sets it as the default for new websites. If the install-time enforcement SQL fails, `post_init_hook` logs the error and re-raises it, so the install fails instead of completing with the banner off. `[@ANCHOR: COMM_compliance_post_init_cookie_bar]`

* **Safe Legal Page Provisioning:** Provisions AGPL-3 compatible legal pages safely via `noupdate="1"` XML records (Odoo does not overwrite `noupdate` records on module update, so site-owner edits survive). `[@ANCHOR: COMM_compliance_legal_pages_rendering]`

    * Privacy Policy Template `[@ANCHOR: COMM_compliance_privacy_policy_template]`

    * Cookie Policy Template `[@ANCHOR: COMM_compliance_cookie_policy_template]`

    * Terms of Service Template `[@ANCHOR: COMM_compliance_terms_of_service_template]`

* **Non-Destructive Mandate:** If, at install time, a page already exists at one of the target URLs (`/privacy`, `/cookie-policy`, `/terms`, `/accessibility`), the module's boilerplate is unpublished to avoid duplication. A page counts as custom when its view key does not start with `compliance.compliance_` and its `website_id` equals the boilerplate's. `[@ANCHOR: COMM_test_compliance_non_destructive_mandate]`
* **Editability Mandate:** Legal pages are standard `website.page` records, allowing administrators to use the Odoo website builder for customization.
* **GDPR Base Contract:** `res.users` carries three base-architecture hooks other modules override
  to participate in a user's GDPR erasure/export. Every override must call `super()` and merge into
  (not replace) the base return shape: callers such as the `/my/privacy/export` controller invoke
  only the most-derived override, and every other module's contribution is reached only through
  the `super()` chain.

  * `_execute_gdpr_erasure()`: deactivates the account, writing as the `zero_sudo.gdpr_service_internal` service account, at the base of the erasure chain. `[@ANCHOR: compliance_execute_gdpr_erasure]`

  * `_get_gdpr_export_data()`: returns an empty dict; overriding modules add their own keys to it. `[@ANCHOR: compliance_get_gdpr_export_data]`

  * `_get_gdpr_streamed_keys()`: returns an empty dict; modules with per-user datasets too large to hold in memory add `{key: generator_function}` entries so the export endpoint can stream each dataset. `[@ANCHOR: compliance_get_gdpr_streamed_keys]`

## 3. API & Integration
### Standardized Routes
Dependent modules requiring legal links MUST use:
* `/privacy` : Privacy Policy
* `/cookie-policy` : Cookie Policy
* `/terms` : Terms of Service
* `/accessibility` : Accessibility Statement

### Integration Rules
1. **Do Not Build Custom Banners:** Rely entirely on Odoo's native `website.cookies_bar`.
2. **Tracking Scripts:** Any third-party JavaScript tracking MUST hook into the Odoo consent state.

### Document Provisioning
Developers can provision new compliance documents using the `compliance.document` model. The `/compliance` portal index page lists up to 100 records whose `active` field is true. Provision them via `noupdate` XML records (as `data/compliance_data.xml` does) so site-owner edits are not overwritten on module update.
</enforcement_details>

<security_architecture>
## 4. Security & Zero-Sudo
This module adheres to **ADR-0002 (Zero-Sudo)** and **ADR-0005 (Service Account Web Isolation)**. ADRs are Architecture Decision Records; these two are now consolidated into `docs/adrs/MASTER_01_SECURITY_ZERO_SUDO.md`. Zero-Sudo means code does not use `.sudo()` or stay as the superuser; it switches to a dedicated, narrowly privileged service account instead.

* **Micro-Privilege Account:** Automated post-install configuration is executed via the `compliance.user_compliance_service` service account, a member of the "Micro-Privilege: Compliance Service" group.
* **ACLs:** The service account's group is granted read/write on `website`; read/write/create on `website.page` and `ir.ui.view`; read-only on `res.groups` and `res.company`; and full access to `compliance.document`. `[@ANCHOR: COMM_compliance_security_acls]`

* **Impersonation:** Escalation is handled via `env(user=svc_uid)` instead of `.sudo()` for core operations. `[@ANCHOR: COMM_compliance_zero_sudo_impersonation]`

## 5. Website-Aware Scope
The module is multi-website aware. When detecting custom pages at target URLs, it only unpublishes a boilerplate page whose `website_id` matches the custom page's (both global, or both the same website); a website-specific custom page does not unpublish the global boilerplate. If a custom page is removed, the boilerplate is re-published the next time `post_init_hook` runs; Odoo runs that hook only when the module is installed, so between installs a site owner re-publishes it manually. `[@ANCHOR: COMM_compliance_website_aware_scope]`

## 6. Documentation Installation
This module declares `knowledge` as a hard dependency in `__manifest__.py` (documentation providers: `knowledge` or Odoo Enterprise `knowledge`), so the documentation provider is always installed with it.

* **Mechanism:** Documentation is automatically provisioned during the final registry reload by the central engine (`_bootstrap_knowledge_docs` in `zero_sudo`, called from `ir.module.module._register_hook`); `post_init_hook` also calls it directly at the end of install. `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]`
* **Article Title:** "Site Owner's Guide to Regulatory Compliance"

## 7. Verification and Testing
Comprehensive test coverage ensures ongoing compliance:
* **Hook Testing:** `test_hooks.py` verifies `cookies_bar` enforcement, non-destructive page provisioning, the `cookies_bar` default for new websites, and that re-running the hook restores the boilerplate once the custom page is gone.
* **Page Integrity:** `test_pages.py` ensures all legal routes are active and contain valid boilerplate content.
* **Security Audit:** `test_security.py` confirms the service account is active, flagged as a service account, and a member of the compliance service group.
* **GDPR Base Contract:** `test_gdpr_base.py` verifies that erasure deactivates the account even when called by a low-privilege user, and that both export hooks return a dict.
* **UI Tours:** `compliance_tour.js` (run by `test_ui_tours.py`) simulates end-to-end user navigation across all legal pages. `[@ANCHOR: COMM_test_compliance_ui_tour]`
</security_architecture>
