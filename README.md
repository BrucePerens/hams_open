# Open Source Community Modules for Odoo 19

Welcome to a comprehensive suite of open-source modules designed for **Odoo 19 Community**. This repository provides tools for scaling horizontally, defending against automated attacks, and building decentralized user communities, all while maintaining rigorous security and developer ergonomics. It also carries a second, independent codebase in the same repo: `daemons/ham_digital_modes`, an LGPL-3.0-or-later suite of amateur radio digital-mode and voice-codec implementations (AMBE/IMBE and Codec2, which compress speech into a small digital stream; WSPR, a weak-signal beacon mode; PSK31 and RTTY, keyboard-to-keyboard text modes; and FT8, a weak-signal contact mode) that has nothing to do with Odoo at all -- see the section below.

**License:** mostly AGPL-3.0-or-later (the GNU Affero General Public License; the GPL and LGPL are the General and Lesser General Public Licenses), but not uniformly -- see [`LICENSING.md`](LICENSING.md) for the real breakdown (a few directories are GPL-3.0-or-later or LGPL-3.0-or-later, one is AGPL-3.0-or-later, and some files carry no header yet) before assuming a license for any specific file.

**Project vocabulary.** `hams_open` is one of the project's repositories, alongside `hams_com` (the proprietary hams.com web site and infrastructure) and [`hams_shared`](hams_shared/README.md) (developer tooling, ADRs and AI-agent skills used by both; it is a git submodule here, and `AGENTS.md`, `tools/` and `docs/` in this repository are symlinks into it). An **ADR** is an Architecture Decision Record: a numbered document recording one standing design decision ([`docs/adrs/`](docs/adrs/)). The **Burn List**, the **Semantic Anchor System**, **Zero-Sudo** and **service accounts** are project-wide terms; each is explained once, in the section below that names it.

---

## 📡 Amateur Radio Digital Modes & Vocoders

`daemons/ham_digital_modes` is a from-scratch (except where noted) suite of amateur radio digital
mode and voice-codec implementations, kept in its own LGPL-3.0-or-later crate specifically so its
decode paths can depend on GPL-3.0 reference code (an LGPL project may incorporate a GPL
dependency; the AGPL/proprietary code elsewhere in this ecosystem cannot). Current state, verified
against the code itself rather than assumed:

* **AMBE/IMBE vocoder** (`ambe/`): the P25 (a public-safety radio standard) and D-STAR (an amateur digital-voice standard) generation of the algorithm, implemented directly
  from TIA-102.BABA (the Telecommunications Industry Association's published description of the P25 vocoder). Both encode and decode are complete, tested, and wired end to end, including
  frame-repeat robustness and spec-accurate comfort-noise generation. The later AMBE+2 generation
  (DMR, Digital Mobile Radio, and Yaesu System Fusion) is deliberately out of scope -- its 2009 addendum names specific
  patents; see `AMBE_PLUS_2_NOTES.md` in the same directory.
* **Codec2** (`codec2_3200/`, `codec2_1600/`): independently-authored Rust ports of Codec2's
  3200bps and 1600bps modes, built to interoperate with real upstream Codec2/Codec2-mod
  bitstreams without being a derivative of Codec2-mod's own LGPL-2.1-only source. The 1600bps mode
  is the speech half of M17's own "Voice + Data" stream type; M17's own framing, forward-error-correction and synchronization layer is
  tracked separately (see `docs/proposals/blocked/M17_IMPLEMENTATION_PLAN.md` in `hams_com`) and is
  not yet built. A no-`std`, no-alloc FFT (fast Fourier transform, a frequency-analysis routine, needing neither Rust's standard library nor heap allocation; `microfft`, replacing an earlier `rustfft` dependency
  that pulled in `std`) keeps both modes cross-compilable to bare-metal and no-FPU (no floating-point unit) targets.
  `daemons/codec2_3200_capi` additionally exposes the 3200bps port through the real upstream
  `codec2.h` C ABI, a link-compatible drop-in for `-lcodec2` for callers (such as an M17 stack)
  that only need that function subset.
* **WSPR** (`wspr.rs`, `wspr_decode.rs`, `wspr_sync.rs`): message encode, audio synthesis, and a
  100%-original sequential (stack-algorithm) decoder for the K=32 rate-1/2 convolutional code (an error-correcting code whose encoder remembers the last 32 bits and emits two output bits per input bit) --
  written from scratch rather than vendored, since every public WSPR decoder traces back to the
  same GPLv3-licensed K1JT/K9AN reference lineage.
* **PSK31** (`psk31.rs`) and **RTTY** (`rtty.rs`): pure Rust, no vendored third-party code, both
  directions implemented for each.
* **FT8** (`ft8.rs`): decode built on vendored `ft8_lib` (Kārlis Goba, MIT-licensed) via a thin C
  shim, verified end to end against real K1JT reference tools (`wsjtx`) across a swept SNR (signal-to-noise ratio) range,
  not just a single clean case.

`daemons/rsp_open` is a separate, AGPL-3.0-or-later Rust library: an open driver for the SDRplay
RSP2pro receiver (a software-defined radio, SDR, from the maker SDRplay) (USB 1df7:3010), written clean-room from the observed USB traffic of the author's
own receiver. It tunes, sets gain, antenna, notch and sample rate, and streams I/Q samples (in-phase and quadrature, the two-part digital form of a radio signal) over
isochronous USB (a USB transfer mode with guaranteed timing), for a receiver whose firmware SDRplay's own software has already loaded (it
contains no firmware and reports a cold receiver as not running). See
[`daemons/rsp_open/README.md`](daemons/rsp_open/README.md) for provenance, scope and test results.

---

## 🤖 Deterministic AI Management & Tooling

Our platform is built to seamlessly integrate Large Language Models (LLMs) into a precise DevSecOps (development, security and operations combined) pipeline. To prevent AI context loss, hallucination, and architectural drift, we govern agents using a strict suite of guidance files and structural memory systems.

### The AI Instruction Suite & Memory
We don't rely on basic system prompts; we govern AI agents using a rigorous hierarchy of operational mandates. What used to be four standalone `docs/LLM_*.md` files has since moved: the universal, always-applicable rules live in `AGENTS.md` itself, and the rest moved into Claude Code's own on-demand [skills](agents/skills/) -- activated automatically when their trigger condition matches, rather than loaded into every session unconditionally.
* **The Agent Persona ([`AGENTS.md`](AGENTS.md)):** The primary entry point defining the AI's boundaries, tone, universal technical standards, and the pre-flight/final-verification protocol every change goes through.
* **The Burn List / Anti-Evasion Reference ([`agents/skills/linter-compliance/`](agents/skills/linter-compliance/SKILL.md)):** An exhaustive, unforgiving AST-based (abstract syntax tree, the parsed structure of code) list of banned patterns, evasion tactics, and deprecated APIs that our custom CI/CD (continuous integration and delivery) linters (`tools/check_burn_list.py`, `tools/verify_anchors.py`) actively block -- activated whenever the AI is writing code.
* **Odoo 19+ Mandates ([`agents/skills/odoo-development/`](agents/skills/odoo-development/SKILL.md)):** Odoo-specific architectural directives (dynamic SQL safety, `psycopg2` parameterization, and more), activated when working on an Odoo module.
* **The AI's own Experience Log ([`agents/skills/project-experience/`](agents/skills/project-experience/SKILL.md)):** A persistent, cross-session memory bank the AI itself curates -- hard-learned lessons and edge cases it chooses to record, consulted when it hits a persistent error or something that looks like a repeat of a past one.
* **Architecture Decision Records ([`docs/adrs/`](docs/adrs/)):** A formal repository of all major structural choices. This acts as the project's long-term memory, ensuring the AI deeply understands the *why* behind our security and performance paradigms.
* The full skill catalog ([`agents/skills/`](agents/skills/)) goes well beyond these four -- Odoo test-harness selection, UI-tour debugging, headless-browser testing, multi-agent codebase review, and more.

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

* **[Zero-Sudo Security Core](zero_sudo/README.md) (`zero_sudo`):** Replaces Odoo's dangerous `.sudo()` method with a centralized Micro-Service Account pattern for least-privilege execution. `.sudo()` runs code with absolute database rights, bypassing access control lists and record rules; under Zero-Sudo it is banned, and an operation that needs elevated rights instead runs as a narrowly scoped **service account** (a user flagged `is_service_account`, which cannot log in through the web) via `with_user()`. The rule is in [MASTER 01](docs/adrs/MASTER_01_SECURITY_ZERO_SUDO.md) and the accounts are catalogued in [`docs/service_accounts.md`](docs/service_accounts.md).
* **[Binary Downloader](binary_downloader/README.md) (`binary_downloader`):** A database-backed module that securely provisions static executables at runtime, validating strict SHA-256 checksums to protect against Arbitrary File Write vulnerabilities.
* **[Cloudflare Edge Orchestration](cloudflare/README.md) (`cloudflare`):** Control your CDN (content delivery network) directly from Odoo to deploy WAF (web application firewall) bans, Zero Trust Tunnels, and Turnstile CAPTCHA (Cloudflare's human-verification challenge).

---

## ⚡ Performance & Scale

Built to handle high traffic and distributed workloads efficiently.

* **[Caching PWA](caching/README.md) (`caching`):** A zero-config PWA (progressive web app) Service Worker (a browser script) that intercepts network requests to act as a client-side CDN for static assets.
* **[Distributed Redis Cache](distributed_redis_cache/README.md) (`distributed_redis_cache`):** A Redis-backed pub/sub (publish/subscribe messaging) bus ensuring fine-grained phase coherence and instant cache invalidation across all Odoo WSGI (the Python web-server interface) nodes. ("Phase coherence": once any worker signals that cached data changed, every other worker discards its copy before it serves its next request or runs its next cron job.)
* **[Database Management & APM (application performance monitoring)](docs/modules/database_management.md) (`database_management`):** An in-GUI DBA (database administrator) toolkit to track table bloat, terminate hanging sessions, and generate HA (high availability) configurations for Patroni (a PostgreSQL failover manager) and PgBouncer (a PostgreSQL connection pooler).

---

## 🚨 Site Reliability Engineering (SRE)

* **[Pager Duty](pager_duty/PROMO.md) (`pager_duty`):** An isolated, Datadog-level Python daemon running outside Odoo's web workers, featuring airgapped SMTP fallbacks, un-cached DNS lookups, and intelligent calendar-based routing. Planned restarts are announced with `sudo pagerduty-maintenance start` so they do not page ([setup and usage](pager_duty/README.md)).
* **[Backup & Disaster Recovery](backup_management/README.md) (`backup_management`):** A centralized GUI orchestrating `Kopia` and `pgBackRest` (two backup programs) with automated restore drills to prove snapshot integrity.

---

## 🌐 Decentralized Community & Content

Empower users while maintaining legal compliance and moderation capabilities.

* **[User Websites](user_websites/README.md) (`user_websites`):** Allows users to build personal or group websites safely using a Proxy Ownership pattern and shared blog container. (Proxy Ownership: a service account performs the database write on the user's behalf, while the user is recorded as the page's owner, so only that user can edit it later; see [`docs/modules/user_websites.md`](docs/modules/user_websites.md).)
* **[Knowledge](knowledge/README.md) (`knowledge`):** A clean-room, open-source replacement for the Knowledge app, enabling hierarchical instruction manuals.
* **[Global Compliance](compliance/README.md) (`compliance`):** Automatically provisions GDPR/CCPA (the European Union and California privacy laws) privacy pages, terms of service, and enforces cookie consent across the ecosystem.



## Working in a git worktree

`hams_shared` is a git submodule, and `git worktree add` leaves it as an EMPTY directory, so inside a worktree
`agents/`, `tools/`, `docs/` and `AGENTS.md` dangle and a test run fails on `import site_monitor` and the like.
Run `python3 fix_worktree_symlinks.py` at the worktree root: it links `hams_shared` to the main checkout's, locally
and marked `skip-worktree` so it cannot be committed. It also sets `fetch.recurseSubmodules=false` in the worktree's own
config (`extensions.worktreeConfig`; the main checkout and other worktrees keep git's default), because git refuses to
fetch into a submodule path that is a symlink: without it every `git fetch`, `git pull` and prmerge.sh run in the worktree
failed with "expected submodule path 'hams_shared' not to be a symbolic link" (tests: `python3 -m unittest test_fix_worktree_symlinks`).
### Anchors for `fix_worktree_symlinks.py`

Code in `fix_worktree_symlinks.py`; tests in `test_fix_worktree_symlinks.py` (`python3 -m unittest test_fix_worktree_symlinks`). The script never signals a process and never touches the network. Its only writes are inside the worktree it runs in (symlinks, `skip-worktree` bits and the worktree's own git config).

### `_git(args, cwd, check=True) -> str` [@ANCHOR: fix_worktree_symlinks:git]

Runs `git` with `args` in `cwd` and returns its standard output stripped of surrounding whitespace. Every other function here goes through it.

| Argument | Type | Meaning |
|---|---|---|
| `args` | list of str | git arguments, without the leading `git` |
| `cwd` | path | the directory git runs in (a worktree root) |
| `check` | bool, default `True` | when true a non-zero exit raises `subprocess.CalledProcessError`; when false the (possibly empty) output is returned and the failure ignored |

Standard error is captured and discarded, so a failing call explains itself only through the exception. Blocking, synchronous, no timeout. Side effects are whatever the git command does (`update-index` and `config` write the index and config of `cwd`). Tests: `test_fix_worktree_symlinks.py` ("Tests" citations for `git`).

### `main_checkout_root(this_root) -> str` [@ANCHOR: fix_worktree_symlinks:main_checkout_root]

Finds the root of the main (non-worktree) checkout that the worktree `this_root` belongs to, however deeply the worktree is nested. It asks `git rev-parse --git-common-dir`, makes a relative answer absolute against `this_root`, normalises it and returns its parent directory. For the main checkout itself the result equals `this_root`, which is how `fix_worktree` detects "nothing to do". Argument `this_root` is the worktree root path. Raises `CalledProcessError` when `this_root` is not inside a git repository. Pure apart from the git read.

### `_is_empty_dir(path) -> bool` [@ANCHOR: fix_worktree_symlinks:is_empty_dir]

True only for a real directory (not a symlink to one) that contains nothing. This is what `git worktree add` leaves in place of a submodule it did not populate. Argument `path` is the candidate path. Returns a bool and never raises for a missing path (it is simply false). Read-only.

### `_is_populated_checkout(path) -> bool` [@ANCHOR: fix_worktree_symlinks:is_populated_checkout]

True when `path` is a directory holding a `.git` entry, file or directory (a submodule working tree has a `.git` file, a normal clone a directory). `fix_worktree` uses it to refuse linking to a main-checkout submodule that was itself never populated. Argument `path` is the candidate directory. Returns a bool; read-only; a missing path is false.

### `_link(this_root, rel_path, target) -> None` [@ANCHOR: fix_worktree_symlinks:link]

Replaces whatever is at `rel_path` in the worktree with a symlink to `target`, then marks the path `skip-worktree` so `git commit -a` or `git add -A` can never commit the absolute, machine-specific target.

| Argument | Meaning |
|---|---|
| `this_root` | worktree root |
| `rel_path` | path of the entry, relative to `this_root` |
| `target` | absolute path the link points at (the main checkout's real path) |

An existing symlink is removed; an existing directory is removed with `os.rmdir`, which only succeeds for an empty one (callers guarantee that, and `OSError` surfaces if they were wrong); then `os.symlink` creates the link and `git update-index --skip-worktree` runs. Returns nothing. Not atomic: a crash between the remove and the link leaves the entry missing, and rerunning the script repairs it (idempotent).

### `_stop_submodule_recursion(this_root) -> None` [@ANCHOR: fix_worktree_symlinks:stop_submodule_recursion]

Makes `git fetch`, `git pull` and `prmerge.sh` in this worktree skip the submodule that was replaced by a symlink. Git's default `fetch.recurseSubmodules=on-demand` walks every gitlink and refuses a symlinked path ("expected submodule path 'hams_shared' not to be a symbolic link"), so every fetch exited non-zero before this was added. Argument `this_root` is the worktree root. It turns on `extensions.worktreeConfig` in the repository config when it is not already `true` (that setting is what allows a per-worktree config file), then writes `fetch.recurseSubmodules=false` with `--worktree`, so the main checkout and other worktrees keep git's default. Idempotent; returns nothing; a git failure raises `CalledProcessError`.

### `_recorded_sha(entry_meta) -> str` [@ANCHOR: fix_worktree_symlinks:recorded_sha]

Extracts the object id from the metadata half of a `git ls-files -s` line (`<mode> <sha> <stage>`): the second whitespace-separated field. For a gitlink this is the submodule commit the worktree records, which `fix_worktree` compares with the main checkout's HEAD to warn about a mismatch. Argument `entry_meta` is that metadata string; returns the sha as a string. Raises `IndexError` on a string with fewer than two fields. Pure.

### `fix_worktree(this_root) -> (int, list[str])` [@ANCHOR: fix_worktree_symlinks:fix_worktree]

The whole repair for one worktree. Returns `(fixed, messages)`: the number of entries it relinked and the human-readable lines to print.

1. If the main checkout root equals `this_root`, returns `(0, ["Running in the main checkout -- nothing to fix."])`.
2. Lists `git ls-files -s`, gitlinks (mode 160000) first, because the tracked symlinks `agents`, `tools`, `docs`, `AGENTS.md` resolve through `hams_shared`.
3. For a gitlink: already a working symlink means it only runs `_stop_submodule_recursion` (heals worktrees linked by an older version) and moves on; a populated directory (a real `git submodule update`) is left alone; if the main checkout's copy is not populated it says so and skips; otherwise it links to the real path of the main copy, stops recursion, counts it, and, when the main copy's HEAD differs from `_recorded_sha`, adds a note that the worktree tests against the main copy's commit.
4. For a tracked symlink (mode 120000) that dangles here: it relinks to the main checkout's resolved target, or reports that it is broken there too.
5. Ends with "No empty submodules or broken cross-repo symlinks found." or a "Fixed N entries locally, uncommitted (do not commit)." line.

Errors: `CalledProcessError` from git. Side effects: only local, uncommitted symlinks, `skip-worktree` bits and the worktree config. Idempotent. Tests: the unit tests cover the main checkout, the empty-submodule and symlink repairs and the recursion setting.

### `main() -> int` [@ANCHOR: fix_worktree_symlinks:main]

Command-line entry point (`python3 fix_worktree_symlinks.py`, run from anywhere inside the worktree). No arguments. It finds the worktree root with `git rev-parse --show-toplevel` from the current directory, runs `fix_worktree`, prints the messages one per line and returns 0 (also when nothing needed fixing; a git failure raises). Test: the `main` test in `test_fix_worktree_symlinks.py`.
