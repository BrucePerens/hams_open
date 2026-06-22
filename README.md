# Open Source Community Modules for Odoo 19

Welcome to a comprehensive suite of open-source modules designed for **Odoo 19 Community**. This repository provides tools for scaling horizontally, defending against automated attacks, and building decentralized user communities, all while maintaining rigorous security and developer ergonomics.

---

## 🤖 Deterministic AI Management & Tooling

Our platform is built to seamlessly integrate Large Language Models (LLMs) into a precise DevSecOps pipeline. To prevent AI context loss, hallucination, and architectural drift, we govern agents using a strict suite of guidance files and structural memory systems.

### The AI Instruction Suite & Memory
We don't rely on basic system prompts; we govern AI agents using a rigorous hierarchy of operational mandates:
* **The Agent Persona ([`AGENTS.md`](AGENTS.md)):** The primary entry point defining the AI's boundaries, tone, and the Exactness Guarantee for file patching.
* **Universal Standards ([`docs/LLM_GENERAL_REQUIREMENTS.md`](docs/LLM_GENERAL_REQUIREMENTS.md)):** Core rules covering code formatting (Black), security patterns, and multi-step execution logic.
* **Odoo 19 Mandates ([`docs/LLM_ODOO_REQUIREMENTS.md`](docs/LLM_ODOO_REQUIREMENTS.md)):** Odoo-specific architectural directives, actively preventing the AI from falling back on legacy Odoo 14-17 training data.
* **The Burn List ([`docs/LLM_LINTER_GUIDE.md`](docs/LLM_LINTER_GUIDE.md)):** An exhaustive, unforgiving list of banned AST structures, evasion tactics, and deprecated APIs that our custom CI/CD linters actively block.
* **Architecture Decision Records ([`docs/adrs/`](docs/adrs/)):** A formal repository of all major structural choices. This acts as the project's long-term memory, ensuring the AI deeply understands the *why* behind our security and performance paradigms.
* **The AI's own Experience File** ([`docs/LLM_EXPERIENCE.md`](docs/LLM_EXPERIENCE.md)): ** The AI's own notes to itself, it chooses what goes in this file. Sometimes, experience will be promoted to more formal documents.

### The Semantic Anchor System
To prevent AI "amnesia" and ensure code, tests, and documentation remain permanently synchronized, the platform utilizes a bidirectional **Semantic Anchor System** (`[@ANCHOR: unique_name]`).
* When an AI generates a business rule or UI view in the code, it drops an anchor.
* That exact anchor must physically appear in the corresponding automated Python or JS test.
* That exact anchor must also be referenced inline within the relevant Markdown documentation.
* Our CI/CD pipeline ([`tools/verify_anchors.py`](tools/verify_anchors.py)) continuously scans the repository. If an AI modifies the code without updating the linked test or documentation, the build mathematically fails, ensuring total architectural traceability.

### Execution & Extraction
* **[Isolated Task Workspaces](tools/create_task_workspace.py):** Surgically partition tasks to prevent LLM cognitive overload and context drift.
* **[MIME-Like Parcel Transport](docs/LLM_GENERAL_REQUIREMENTS.md):** Code modifications are delivered with absolute precision using a secure, multi-block transport schema (requiring strict 4-backtick encapsulation to protect AST formatters).
* **[Semantic Token Matching](tools/parcel_extract.py):** A patching engine that ignores superficial whitespace and formatting, ensuring AI-generated search-and-replace blocks dock perfectly with the source code.
* **[True Environment Parity](test_real_transaction/README.md) (`test_real_transaction`):** A testing facility that bypasses Odoo's test cursor wrapper for true database commits and cross-worker behavior testing.

---

## 🛡️ Security & Edge Defense

Security is mathematically enforced at the lowest levels of the architecture.

* **[Zero-Sudo Security Core](zero_sudo/README.md) (`zero_sudo`):** Replaces Odoo's dangerous `.sudo()` method with a centralized Micro-Service Account pattern for least-privilege execution.
* **[Binary Downloader](binary_downloader/README.md) (`binary_downloader`):** A database-backed module that securely provisions static executables at runtime, validating strict SHA-256 checksums to protect against Arbitrary File Write vulnerabilities.
* **[Cloudflare Edge Orchestration](cloudflare/README.md) (`cloudflare`):** Control your CDN directly from Odoo to deploy WAF bans, Zero Trust Tunnels, and Turnstile CAPTCHA.

---

## ⚡ Performance & Scale

Built to handle high traffic and distributed workloads efficiently.

* **[Caching PWA](caching/README.md) (`caching`):** A zero-config Service Worker that intercepts network requests to act as a client-side CDN for static assets.
* **[Distributed Redis Cache](distributed_redis_cache/README.md) (`distributed_redis_cache`):** A Redis-backed pub/sub bus ensuring fine-grained phase coherence and instant cache invalidation across all Odoo WSGI nodes.
* **[Database Management & APM](docs/modules/database_management.md) (`database_management`):** An in-GUI DBA toolkit to track table bloat, terminate hanging sessions, and generate HA configurations for Patroni and PgBouncer.

---

## 🚨 Site Reliability Engineering (SRE)

* **[Pager Duty](pager_duty/PROMO.md) (`pager_duty`):** An isolated, Datadog-level Python daemon running outside Odoo's web workers, featuring airgapped SMTP fallbacks, un-cached DNS lookups, and intelligent calendar-based routing.
* **[Backup & Disaster Recovery](backup_management/README.md) (`backup_management`):** A centralized GUI orchestrating `Kopia` and `pgBackRest` with automated restore drills to prove snapshot integrity.

---

## 🌐 Decentralized Community & Content

Empower users while maintaining legal compliance and moderation capabilities.

* **[User Websites](user_websites/README.md) (`user_websites`):** Allows users to build personal or group websites safely using a Proxy Ownership pattern and shared blog container.
* **[Knowledge](knowledge/README.md) (`knowledge`):** A clean-room, open-source replacement for the Knowledge app, enabling hierarchical instruction manuals.
* **[Global Compliance](compliance/README.md) (`compliance`):** Automatically provisions GDPR/CCPA privacy pages, terms of service, and enforces cookie consent across the ecosystem.

