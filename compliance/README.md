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
* How our server-side view counters work.
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
python3 hams_shared/tools/test.py --wait -u compliance
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
* [Automatic Legal Pages Generation](./docs/stories/automatic_legal_pages.md) `[@ANCHOR: COMM_story_automatic_legal_pages]`: the story of a site owner who installs the module and finds the Privacy Policy, Cookie Policy, Terms of Service and Accessibility Statement already published at their standard URLs, linked from every footer and listed on `/compliance`, with her own pages left untouched. Verified by `test_pages.py` and `test_hooks.py`.

* [Enforced Cookie Consent](./docs/stories/cookie_consent.md) `[@ANCHOR: COMM_story_cookie_consent]`: the story of a visitor who sees Odoo's native cookie bar on first load because `post_init_hook` switched `website.cookies_bar` on for every website, and who can read the Cookie Policy from the bar. The module does not itself block scripts that ignore the bar. Verified by `test_02_post_init_hook_cookie_bar` and the UI tour.

* [Site Owner Documentation](./docs/stories/compliance_documentation.md) `[@ANCHOR: COMM_story_compliance_documentation]`: the story of a site owner who finds the "Site Owner's Guide to Regulatory Compliance" article in the Knowledge app after install. `post_init_hook` calls `_bootstrap_knowledge_docs` of `zero_sudo` as the compliance service account, which loads `data/documentation.html` as that article; the story file lists what the guide covers.

#### Journeys
* [Compliance Setup Journey](./docs/journeys/compliance_setup_journey.md) `[@ANCHOR: COMM_journey_compliance_setup]`: the end-to-end path of installing the module: the hook enables the cookie bar, the legal page templates and records are created or left alone, and the guide is added to Knowledge. It is the anchor the `post_init_hook` in `hooks.py` carries, and the UI tour `compliance_tour.js` walks it.

## 2. Enforcement Details
* **Automated Cookie Consent:** Programmatically enables the Odoo `website` native `cookies_bar` boolean on install and sets it as the default for new websites. If the install-time enforcement SQL fails, `post_init_hook` logs the error and re-raises it, so the install fails instead of completing with the banner off. `[@ANCHOR: COMM_compliance_post_init_cookie_bar]`

* **Safe Legal Page Provisioning:** Provisions AGPL-3 compatible legal pages safely via `noupdate="1"` XML records (Odoo does not overwrite `noupdate` records on module update, so site-owner edits survive). `[@ANCHOR: COMM_compliance_legal_pages_rendering]` The records live in `data/legal_pages_data.xml`: one QWeb template per page (`compliance_privacy_policy_template`, `compliance_cookie_policy_template`, `compliance_terms_of_service_template`, `compliance_accessibility_statement_template`) and one `website.page` record per URL (`/privacy`, `/cookie-policy`, `/terms`, `/accessibility`), published, plus `compliance.document` index rows in `data/compliance_data.xml`. Each template calls the shared `compliance_legal_links` template at its foot. Test: `test_03_views_rendering` checks that the three policy templates build their combined arch without error.

    * Privacy Policy Template `[@ANCHOR: COMM_compliance_privacy_policy_template]`: the QWeb view `compliance.compliance_privacy_policy_template`, served at `/privacy`. It has five numbered sections (Introduction; Data We Collect; How We Use Your Data; Your Regulatory Rights, GDPR, CCPA and similar; Community Safety and Reporting) and a disclaimer that it is a default version a site owner must review. A site module may replace the wording by inheriting the view. Tests: `test_pages_presence`, `test_pages_content`.

    * Cookie Policy Template `[@ANCHOR: COMM_compliance_cookie_policy_template]`: the QWeb view `compliance.compliance_cookie_policy_template`, served at `/cookie-policy` and linked from the cookie bar. Three sections: what cookies are, how this platform uses them, and how a visitor manages preferences through the native Odoo consent bar. It is the page a visitor reads before accepting cookies. Tests: `test_pages_presence`, `test_pages_content`, the UI tour.

    * Terms of Service Template `[@ANCHOR: COMM_compliance_terms_of_service_template]`: the QWeb view `compliance.compliance_terms_of_service_template`, served at `/terms`. Five sections: acceptance of terms, user-generated content and ownership, community guidelines and moderation, open source licensing, and limitation of liability. Like the other templates it ships as editable boilerplate with a default-version notice. Tests: `test_pages_presence`, `test_pages_content`, the UI tour.

* **Non-Destructive Mandate:** If, at install time, a page already exists at one of the target URLs (`/privacy`, `/cookie-policy`, `/terms`, `/accessibility`), the module's boilerplate is unpublished to avoid duplication. A page counts as custom when its view key does not start with `compliance.compliance_` and its `website_id` equals the boilerplate's. `[@ANCHOR: COMM_test_compliance_non_destructive_mandate]`
* **Editability Mandate:** Legal pages are standard `website.page` records, allowing administrators to use the Odoo website builder for customization.
* **GDPR Base Contract:** `res.users` carries three base-architecture hooks other modules override
  to participate in a user's GDPR erasure/export. Every override must call `super()` and merge into
  (not replace) the base return shape: callers such as the `/my/privacy/export` controller invoke
  only the most-derived override, and every other module's contribution is reached only through
  the `super()` chain.

  * `_execute_gdpr_erasure()`: deactivates the account, writing as the `zero_sudo.gdpr_service_internal` service account, at the base of the erasure chain. `[@ANCHOR: compliance_execute_gdpr_erasure]` Signature `res.users._execute_gdpr_erasure(self) -> None`; it takes no arguments besides the record, calls `ensure_one()` (a multi-record or empty set raises `ValueError`) and writes `active = False` with `with_user(svc_uid)`, so the caller needs no write right of its own (`security/ir.model.access.csv` grants the service group write on `res.users`). Overriding modules do their own hard deletion first and must call `super()`. Domain scrubbing of names or callsigns stays in the modules that own that data. Tests: `test_gdpr_base.py` (deactivation, and that the service account, not the caller, writes).

  * `_get_gdpr_export_data()`: returns an empty dict; overriding modules add their own keys to it. `[@ANCHOR: compliance_get_gdpr_export_data]` Signature `res.users._get_gdpr_export_data(self) -> dict`; no arguments, no side effects, no database access in the base. Each override calls `super()` and adds `{key: JSON-serialisable value}` entries for its own data, merging into the result rather than replacing it, because the `/my/privacy/export` controller calls only the most-derived override. Test: `test_gdpr_base.py` asserts the base returns a dict.

  * `_get_gdpr_streamed_keys()`: returns an empty dict; modules with per-user datasets too large to hold in memory add `{key: generator_function}` entries so the export endpoint can stream each dataset. `[@ANCHOR: compliance_get_gdpr_streamed_keys]` Signature `res.users._get_gdpr_streamed_keys(self) -> dict`; no arguments and no side effects in the base. The value for each key is a generator function the export endpoint calls and writes out item by item, so a large QSO (radio contact) log never sits in memory whole. Overrides call `super()` and add to the dict. Test: `test_gdpr_base.py` asserts the base returns a dict.

* **Pages that are not boilerplate.** Besides the four standard legal pages, the module ships three pages written for this
  deployment. Each is a `website.page` plus a `compliance.document` record, so it is listed on `/compliance` and every
  visitor, signed in or not, can read it.

  * **Our Certification Authority** (`/compliance/certification-authority`) explains why it is safe to accept the hams.com root
    certificate: the CA is limited inside the certificates themselves to hams.com and local-network names, its signing keys sit
    in a Google HSM that nobody can read, and the page says what a thief of the signing permission could and could not do. It
    lists the permitted names (hams.com, .local, .lan, .home.arpa, .internal and the private address ranges), and a test checks the page names each of them. `[@ANCHOR: compliance:certification_authority_page]` The page is the QWeb template `certification_authority_template` in `data/certification_authority_data.xml` with a published `website.page` and a `compliance.document` row (`doc_certification_authority`), so `/compliance` lists it. Its sections are what the CA is for, why accepting the root is safe, how the signing keys are held, what an attacker could and could not do, lifetimes and revocation, and how to verify what you are trusting. Test: `test_certification_authority_page_reachable_and_registered`.

  * **DMCA Notice and Designated Agent** (`/compliance/dmca`) gives the DMCA notice procedure, the counter-notice steps and the
    repeat-infringer policy. The registration number and agent name are per deployment (this code is open source and other sites
    run it), so they are read from the system parameters `compliance.dmca_registration_number` and `compliance.dmca_agent_name`,
    never written into the template; the postal address, phone and email come from the company record. A site with no
    registration number says no agent is registered and invents none. The page's template is `[@ANCHOR: compliance:dmca_page]`
    and its controller route, which reads the parameters bypassing the cache so a change shows at once, is
    `[@ANCHOR: compliance:dmca_page_route]`. The template `dmca_template` (in `data/dmca_data.xml`, with a `compliance.document` row `doc_dmca`) has the sections: our designated agent, how to send a notice, what we do on a valid notice, counter-notice, false notices and repeat infringers. The route is `ComplianceController.compliance_dmca(self)`, `GET /compliance/dmca`, `auth="public"`, `website=True`, no arguments: it reads both parameters with `zero_sudo.security.utils._get_system_param(name, '')` under `with_context(redis_bypass_cache=True)`, strips them, and renders `compliance.dmca_template` with `dmca_registration_number` and `dmca_agent_name` (empty strings when unset, which the page words as no agent registered). It writes nothing. Tests: `test_dmca_page_reads_registration_and_company_contact`, `test_dmca_page_without_registration_says_none_registered`, `test_dmca_page_listed_in_compliance_index`.

  * **Intellectual Property** (`/intellectual-property`) records the filed USPTO provisional patent applications by serial number,
    the project's awareness of related third-party patent activity, and its open-source publication status.
    `[@ANCHOR: compliance:intellectual_property_page]` The page is the QWeb template in `data/intellectual_property_data.xml`, with a published `website.page` record and a `compliance.document` row (`doc_intellectual_property`) so `/compliance` lists it; it has three sections (filed provisional applications, awareness of third-party activity, open source publication) and is public, static text with no controller of its own. Test: `test_intellectual_property_page_reachable_and_registered`.

* **Page edits reach installed databases.** The certification authority template used to sit in a `noupdate="1"` data block, so a
  module update skipped it and the 2026-10-12 rewrite of the page never reached production. The template now sits in an ordinary
  data block, and the 1.2 pre-migration clears the `noupdate` flag its existing `ir_model_data` row still carried, before the
  same update reloads the data files. Nothing is changed on a fresh install. `[@ANCHOR: compliance:clear_stale_ca_template_noupdate_flag]` Signature `migrate(cr, version)` in `migrations/1.2/pre-migration.py`: `cr` is the upgrade cursor, `version` the installed version (empty on a fresh install, where it returns at once). It runs one static `UPDATE ir_model_data SET noupdate = false` for the xmlid `certification_authority_template` in module `compliance`, only where the flag is still set, so it is idempotent, touches nothing else and returns `None`. Test: `tests/test_noupdate_migration.py`.

* **The accessibility statement reaches installed databases too.** Its template (`compliance_accessibility_statement_template`) sat in a
  `noupdate="1"` block, so after the 2026-11 rewrite ("Display and Reading Choices") a module update left the old text in place; a
  rehearsal of the production upgrade on a restored copy of `hams_prod` found it. The template now sits in an ordinary data block
  (the page record stays in the noupdate block), and the 1.3 pre-migration clears the `noupdate` flag its existing `ir_model_data` row
  still carried, before the same update reloads the data files. Argument: `cr` (the migration cursor) and `version` (the installed
  version, empty on a fresh install, where nothing is changed). It is plain SQL, touches only the one listed xmlid, and is
  idempotent. `[@ANCHOR: compliance:clear_stale_accessibility_template_noupdate_flag]` Signature `migrate(cr, version)` in `migrations/1.3/pre-migration.py`; it runs one static `UPDATE ir_model_data SET noupdate = false` for the xmlid `compliance_accessibility_statement_template` of module `compliance` where the flag is still set, returns `None`, and does nothing on a fresh install (empty `version`). Test: `tests/test_noupdate_migration.py`.

* **Accessible cookie banner.** Odoo's stock cookie bar is a `role="dialog"` with no name, so a screen reader announced only
  "dialog" (axe-core `aria-dialog-name`, serious, on every page). The module's inherited template gives it the accessible name
  "Cookie preferences". `[@ANCHOR: compliance:COMM_compliance_cookies_bar_name]` The template `compliance_cookies_bar_name` in `views/website_templates.xml` inherits `website.cookies_bar` and, with an XPath on the `o_cookies_discrete` div, sets its `aria-label` attribute to `Cookie preferences`. It changes no behaviour, only the accessible name, and has no arguments. Test: `test_cookies_bar_dialog_has_an_accessible_name` (anchor `COMM_test_cookies_bar_has_a_name`).

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
* **ACLs:** The service account's group is granted read/write on `website`; read/write/create on `website.page` and `ir.ui.view`; read-only on `res.groups` and `res.company`; and full access to `compliance.document`. `[@ANCHOR: COMM_compliance_security_acls]` The group is `compliance.group_compliance_service` ("Micro-Privilege: Compliance Service", in the privilege "Global Compliance"), declared in `security/security_data.xml`; the rules are in `security/ir.model.access.csv`. Public, portal and internal users may only read `compliance.document`. The group also lets the GDPR service group write `res.users` for the erasure base contract. No row grants delete on website, page or view. Test: `test_security.py`.

* **Impersonation:** Escalation is handled via `env(user=svc_uid)` instead of `.sudo()` for core operations. `[@ANCHOR: COMM_compliance_zero_sudo_impersonation]` In `hooks.post_init_hook(env)` the service user's id comes from `zero_sudo.security.utils._get_service_uid("compliance.user_compliance_service")` (an `AccessError` if the account is missing or not a service account) and the whole hook runs on `env(user=svc_uid)`, so every statement is checked against that account's narrow rights and the superuser is never used. Test: `test_service_account` in `test_security.py` and the hook tests.

## 5. Website-Aware Scope
The module is multi-website aware. When detecting custom pages at target URLs, it only unpublishes a boilerplate page whose `website_id` matches the custom page's (both global, or both the same website); a website-specific custom page does not unpublish the global boilerplate. If a custom page is removed, the boilerplate is re-published the next time `post_init_hook` runs; Odoo runs that hook only when the module is installed, so between installs a site owner re-publishes it manually. `COMM_compliance_website_aware_scope`

## 6. Documentation Installation
This module declares `knowledge` as a hard dependency in `__manifest__.py` (documentation providers: `knowledge` or Odoo Enterprise `knowledge`), so the documentation provider is always installed with it.

* **Mechanism:** Documentation is automatically provisioned during the final registry reload by the central engine (`_bootstrap_knowledge_docs` in `zero_sudo`, called from `ir.module.module._register_hook`); `post_init_hook` also calls it directly at the end of install. `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]` This module only calls it: `post_init_hook` runs `env_svc["ir.module.module"]._bootstrap_knowledge_docs()` as the compliance service account after the cookie-bar procedure, so the article "Site Owner's Guide to Regulatory Compliance" (from `data/documentation.html`) exists as soon as install finishes. The engine's own arguments, returns and errors are documented with `zero_sudo`; an exception it raises fails the install.
* **Article Title:** "Site Owner's Guide to Regulatory Compliance"

## 7. Verification and Testing
Comprehensive test coverage ensures ongoing compliance:
* **Hook Testing:** `test_hooks.py` verifies `cookies_bar` enforcement, non-destructive page provisioning, the `cookies_bar` default for new websites, and that re-running the hook restores the boilerplate once the custom page is gone.
* **Page Integrity:** `test_pages.py` ensures all legal routes are active and contain valid boilerplate content.
* **Security Audit:** `test_security.py` confirms the service account is active, flagged as a service account, and a member of the compliance service group.
* **GDPR Base Contract:** `test_gdpr_base.py` verifies that erasure deactivates the account even when called by a low-privilege user, and that both export hooks return a dict.
* **UI Tours:** `compliance_tour.js` (run by `test_ui_tours.py`) simulates end-to-end user navigation across all legal pages. `[@ANCHOR: COMM_test_compliance_ui_tour]` The test opens `/privacy` and starts the tour `compliance_tour` at `/en_US/privacy?debug=1`; the tour checks the Privacy Policy heading, that the footer carries the Privacy, Cookie Policy, Terms, Accessibility and My Privacy links, then clicks the Cookie Policy link and checks that page loads. It needs a browser (headless Chrome through Odoo's `HttpCase`) and a database with the module installed; it changes only the test database.
</security_architecture>
