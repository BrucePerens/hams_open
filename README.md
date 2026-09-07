# Open Source Community Modules for Odoo 19

Welcome to a comprehensive suite of open-source modules designed for **Odoo 19 Community**. This repository provides tools for scaling horizontally, defending against automated attacks, and building decentralized user communities, all while maintaining rigorous security and developer ergonomics. It also carries a second, independent codebase in the same repo: `daemons/ham_digital_modes`, an LGPL-3.0-or-later suite of amateur radio digital-mode and voice-codec implementations (AMBE/IMBE, Codec2, WSPR, PSK31, RTTY, FT8) that has nothing to do with Odoo at all -- see the section below.

**License:** mostly AGPL-3.0-or-later, but not uniformly -- see [`LICENSING.md`](LICENSING.md) for the real breakdown (a few directories are GPL-3.0-or-later or LGPL-3.0-or-later, one is AGPL-3.0-or-later, and some files carry no header yet) before assuming a license for any specific file.

---

## 📡 Amateur Radio Digital Modes & Vocoders

`daemons/ham_digital_modes` is a from-scratch (except where noted) suite of amateur radio digital
mode and voice-codec implementations, kept in its own LGPL-3.0-or-later crate specifically so its
decode paths can depend on GPL-3.0 reference code (an LGPL project may incorporate a GPL
dependency; the AGPL/proprietary code elsewhere in this ecosystem cannot). Current state, verified
against the code itself rather than assumed:

* **AMBE/IMBE vocoder** (`ambe/`): the P25/D-STAR generation of the algorithm, implemented directly
  from TIA-102.BABA. Both encode and decode are complete, tested, and wired end to end, including
  frame-repeat robustness and spec-accurate comfort-noise generation. The later AMBE+2 generation
  (DMR, Yaesu System Fusion) is deliberately out of scope -- its 2009 addendum names specific
  patents; see `AMBE_PLUS_2_NOTES.md` in the same directory.
* **Codec2** (`codec2_3200/`, `codec2_1600/`): independently-authored Rust ports of Codec2's
  3200bps and 1600bps modes, built to interoperate with real upstream Codec2/Codec2-mod
  bitstreams without being a derivative of Codec2-mod's own LGPL-2.1-only source. The 1600bps mode
  is the speech half of M17's own "Voice + Data" stream type; M17's own framing/FEC/sync layer is
  tracked separately (see `docs/proposals/blocked/M17_IMPLEMENTATION_PLAN.md` in `hams_com`) and is
  not yet built. A no-`std`, no-alloc FFT (`microfft`, replacing an earlier `rustfft` dependency
  that pulled in `std`) keeps both modes cross-compilable to bare-metal/no-FPU targets.
  `daemons/codec2_3200_capi` additionally exposes the 3200bps port through the real upstream
  `codec2.h` C ABI, a link-compatible drop-in for `-lcodec2` for callers (such as an M17 stack)
  that only need that function subset.
* **WSPR** (`wspr.rs`, `wspr_decode.rs`, `wspr_sync.rs`): message encode, audio synthesis, and a
  100%-original sequential (stack-algorithm) decoder for the K=32 rate-1/2 convolutional code --
  written from scratch rather than vendored, since every public WSPR decoder traces back to the
  same GPLv3-licensed K1JT/K9AN reference lineage.
* **PSK31** (`psk31.rs`) and **RTTY** (`rtty.rs`): pure Rust, no vendored third-party code, both
  directions implemented for each.
* **FT8** (`ft8.rs`): decode built on vendored `ft8_lib` (Kārlis Goba, MIT-licensed) via a thin C
  shim, verified end to end against real K1JT reference tools (`wsjtx`) across a swept SNR range,
  not just a single clean case.

---

## 🤖 Deterministic AI Management & Tooling

Our platform is built to seamlessly integrate Large Language Models (LLMs) into a precise DevSecOps pipeline. To prevent AI context loss, hallucination, and architectural drift, we govern agents using a strict suite of guidance files and structural memory systems.

### The AI Instruction Suite & Memory
We don't rely on basic system prompts; we govern AI agents using a rigorous hierarchy of operational mandates:
* **The Agent Persona ([`AGENTS.md`](AGENTS.md)):** The primary entry point defining the AI's boundaries, tone, universal technical standards, and the pre-flight/final-verification protocol every change goes through.
* **The Burn List ([`tools/check_burn_list.py`](tools/check_burn_list.py)):** An exhaustive, unforgiving AST-based list of banned patterns, evasion tactics, and deprecated APIs that our custom CI/CD linters actively block -- read its own module docstring first if this linter fails your code.
* **Architecture Decision Records ([`docs/adrs/`](docs/adrs/)):** A formal repository of all major structural choices. This acts as the project's long-term memory, ensuring the AI deeply understands the *why* behind our security and performance paradigms.

### The Semantic Anchor System
To prevent AI "amnesia" and ensure code, tests, and documentation remain permanently synchronized, the platform utilizes a bidirectional **Semantic Anchor System** (`[@ANCHOR: unique_name]`).
* When an AI generates a business rule or UI view in the code, it drops an anchor.
* That exact anchor must physically appear in the corresponding automated Python or JS test.
* That exact anchor must also be referenced inline within the relevant Markdown documentation.
* Our CI/CD pipeline ([`tools/verify_anchors.py`](tools/verify_anchors.py)) continuously scans the repository. If an AI modifies the code without updating the linked test or documentation, the build mathematically fails, ensuring total architectural traceability.

### Execution & Extraction
* **True Environment Parity** ([`zero_sudo/tests/real_transaction.py`](zero_sudo/tests/real_transaction.py), `RealTransactionCase`): A testing facility that bypasses Odoo's test cursor wrapper for true database commits and cross-worker behavior testing.

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

