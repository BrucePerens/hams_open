---
anchor: cron_notify_pending_reports
code_hash: sha256:04e20e0b0bebe708be087e1ac85bbe763475fbc1288a02fae72a2b8be97bbdd6
---

# Claim: `ContentViolationReport._cron_notify_pending_reports`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
found this function's real behavior, once every sub-call is traced to its actual implementation,
diverges sharply from what its own `# burn-ignore-company-scoped-loop` comment claims. Phrased in
EARS, extended with the FOR-EACH pattern (this claim is the pattern's first real-world use after
its own design). The statements below describe the *verified* behavior as it actually executes,
not the behavior the comment asserts -- see `hams_shared/agents/skills/bug-hunt/SKILL.md`'s bug
class 15 and the accompanying bug-hunt report for why the mismatches matter.

FOR-EACH `company` IN `res.company` (every company in the database, from an unfiltered
`self.env["res.company"].search([], limit=10000)`, in whatever order that search returns --
not restricted to companies with pending reports, and not restricted to any single company):

1. IF `company != base.main_company`, THEN THE system SHALL raise `odoo.exceptions.AccessError`,
   uncaught, when evaluating the pending-report count under `.with_company(company)` for service
   account `user_websites.user_websites_service_account` (whose own `company_ids`, per
   `user_websites/data/user_websites_data.xml`, contains only `base.main_company`) -- terminating
   the FOR-EACH loop entirely for that scheduled run and skipping every company from that point in
   iteration order onward, including ones with real, unprocessed pending reports.
2. WHEN `company == base.main_company`, THE system SHALL count `content.violation.report` records
   in state `"new"` -- but THE system SHALL NOT restrict that count to `base.main_company`'s own
   records: `content_violation_report_admin_rule`'s own `[(1, '=', 1)]` domain OR-dominates the
   multi-company `ir.rule` (Odoo combines same-model group-scoped rule domains with OR), so the
   count reflects every company's `"new"` reports in the database, not just this one's.
3. IF that count is greater than zero AND `email_template_pending_violations_summary` resolves
   (`self.env.ref(..., raise_if_not_found=False)`), THEN THE system SHALL determine a recipient
   address as: system parameter `company_abuse_email`, read via a `.with_company(company)` call
   that has no actual per-company effect (the backing `ir.config_parameter` row is a single global
   value, not company-scoped, so every company that reaches this line receives the identical
   address); IF that is unset or empty, THEN the company's own `email` field (the only genuinely
   per-company fallback in this chain); IF that is also unset or empty, THEN the literal string
   `"admin@example.com"`.
4. IF `email_template_pending_violations_summary` does not resolve, THEN THE system SHALL send no
   email and log or raise nothing, for that company.
5. WHEN a recipient address has been determined per statement 3, THE system SHALL send the
   template's email under service account `zero_sudo.mail_service_internal` (itself
   `.with_company(company)`-scoped, and independently subject to the same AccessError mechanism as
   statement 1 for any company other than `base.main_company` -- in practice unreachable for any
   other company, since statement 1's own count call already raises first), with
   `force_send=False`, and with `email_values={"email_to": abuse_email}` overriding the template's
   own unpopulated `{{ ctx.get('email_to') }}` expression (`mail.template.send_mail_batch`'s
   `values.update(email_values or {})`).
6. THE system SHALL NOT catch any exception raised by statements 1 or 5 anywhere in this
   function's own body (no `try`/`except` exists here); the first `AccessError` encountered while
   iterating `companies` propagates to the `ir.cron` dispatch machinery, terminating that
   scheduled run.
7. IF `_get_service_uid` cannot resolve `svc_uid` or `mail_svc` to an active, correctly-flagged
   service account, THEN THE system SHALL raise (fail loudly), not return a falsy value.

**ISOLATION: NOT maintained** -- confirmed by reading, not assumed. This FOR-EACH loop's own
`.with_company()` scoping does not actually isolate one company's processing from another's
outcome in either direction: (a) a company other than `base.main_company` causes an uncaught
`AccessError` (statement 1) that aborts processing for every company still to come in iteration
order -- a same-run, cross-company *failure* dependency the comment's own "lets each company's
records become visible" framing does not disclose; and (b) `base.main_company`'s own count
(statement 2) is not isolated from other companies' data in the first place -- a cross-tenant
*read* leak, not a write, but a real violation of this codebase's own deliberately-hardened
multi-company data protection (`MASTER_16_FINANCIAL_DATA_PROTECTION.md`) nonetheless.

**Root cause, and the fix that actually respects that hardening (corrected from this claim's own
first draft)**: the loop's own iteration domain (statement 0, `res.company.search([], limit=10000)`
-- every company in the database) is broader than the service account's actual legitimate scope
(its own `company_ids`, containing only `base.main_company`). Per ADR-0083, per-company
`.with_company()` context-switching is the *mandated* pattern for cron jobs touching multi-company
records -- that part of the design is correct and should not be removed. The bug is iterating over
every company that exists rather than over the set the service account is actually entitled to
serve: the fix is `for company in svc_user.company_ids:` (deriving the loop's own domain from the
account's real, provisioned scope), not collapsing to a single ungrouped/grouped query. A single
query was this claim's own first-draft suggestion and is wrong: it would depend on
`content_violation_report_admin_rule`'s own `[(1, '=', 1)]` domain to see across companies at all,
which is itself the accidental, over-permissive rule statement 2 already found -- building the fix
on top of it would turn today's *partial* leak (one company's count, and only when the loop
happens to reach it before crashing) into a *complete* one (every company's count, in every run,
reliably). If the intended design is for this cron to serve every company, the service account's
own `company_ids` needs to be deliberately, correctly provisioned (and kept in sync as new
companies are created) to include them -- a real, separate provisioning question, not a query
rewrite -- and `content_violation_report_admin_rule` itself likely needs to be scoped to
`company_id` rather than left unconditionally permissive, matching the deliberate privacy
tightening this rule currently undermines.
