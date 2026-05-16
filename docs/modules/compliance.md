# 🗄️ Global Compliance Module (`compliance`)

*Copyright © Bruce Perens K6BP. AGPL-3.0.*

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
* Programmatically enables the Odoo `website` native `cookies_bar` boolean. `[@ANCHOR: compliance_post_init_cookie_bar]`
* Provisions AGPL-3 compatible legal pages (`/privacy`, `/cookie-policy`, `/terms`) safely via `noupdate="1"` XML records.
    * Privacy Policy Template `[@ANCHOR: compliance_privacy_policy_template]`
    * Cookie Policy Template `[@ANCHOR: compliance_cookie_policy_template]`
    * Terms of Service Template `[@ANCHOR: compliance_terms_of_service_template]`
* **CRITICAL:** Custom modules MUST NOT implement custom cookie banners. They must utilize the core framework's consent state.
</enforcement_details>

<security_architecture>
## 3. Security & Zero-Sudo
This module adheres to **ADR-0002 (Zero-Sudo)** and **ADR-0005 (Service Account Web Isolation)**.

* **Micro-Privilege Account:** Automated post-install configuration is executed via the `compliance.user_compliance_service` service account.
* **ACLs:** The service account is granted minimal read/write access to `website`, `website.page`, and `ir.ui.view` models.
* **Impersonation:** Escalation is handled via `env.with_user(svc_uid)` instead of `.sudo()` for core operations.
</security_architecture>
