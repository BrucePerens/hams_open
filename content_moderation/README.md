# Content Moderation

A content-type-agnostic report/track/enforce pipeline: `content.violation.report`.

Extracted 2026-09-23 out of `user_websites`, which used to own a version of
this same mechanism hard-wired to its own personal/group website suspension
logic. See `models/content_violation_report.py` for the full extraction
rationale and `user_websites/models/content_violation_report_moderation.py`
(in the `user_websites` module) for the reference override.

## Model (`models/content_violation_report.py`)

* **`content.violation.report`**: `target_url` (plain `Char`, not a hard
  foreign key to any content-specific model), `description`,
  `reported_by_user_id`/`reported_by_email`, `content_owner_id` (a plain
  `res.users` reference -- "who to blame for this content"), `company_id`,
  and a `state` machine (`new` -> `under_review` / `action_taken` /
  `dismissed`). Unique per `(target_url, reported_by_user_id)`.
* **`action_mark_under_review()`**, **`action_dismiss()`**,
  **`action_take_action_and_strike()`**: the moderator workflow actions,
  each with a server-side state guard (a resolved report cannot be
  re-processed by a stale client tab or a direct RPC).
* **`_apply_enforcement_action()`**: the extension hook. Called once per
  report by `action_take_action_and_strike()`, after `state` is already
  `action_taken`. This module's own default is a safe no-op (a chatter
  note, no consequence) -- see that method's own docstring for exactly why
  a "generic" strike-via-stored-procedure default was considered and ruled
  out. A consuming module overrides it via `_inherit =
  "content.violation.report"` to apply its own real consequence.

## What stayed out on purpose

* **`content_group_id`** and any other target-type-specific field: added by
  the consuming module via `_inherit`, not carried here (this module has no
  way to know what a "group" means to every future consumer).
* **`content.violation.appeal`**: stayed in `user_websites`. Both of its
  action methods call `user_websites`-specific pardon methods directly, and
  there's no second consumer yet to justify a generic un-enforcement hook
  the way `content.violation.report` got an enforcement one. A mechanical,
  well-precedented follow-up when one shows up.
* **"Notify moderators of a pending-report backlog"** (cron + mail
  templates + abuse-email config parameter): stayed in `user_websites` too,
  for the same reason -- its naming (`user_websites.company_abuse_email`,
  `user_websites.user_websites_service_account`) is that module's own, and
  genericizing a notification feature nobody else needs yet isn't worth the
  churn.

## Security

* **Moderator** (`group_content_moderation_moderator`): unconditional,
  cross-company access to every report -- mirrors the equivalent role this
  replaced in `user_websites`.
* **Service Account: Content Moderation**
  (`group_content_moderation_service_account`): company-scoped access, for
  a consuming module's own automation/controller account. Deliberately NOT
  implied by the moderator group and NOT given its unconditional rule --
  see `security/content_moderation_security.xml`'s own comments for the
  cross-company leak this avoids (a real bug, previously shipped and fixed
  in `user_websites` before this extraction, preserved-fixed here rather
  than reintroduced).
* Any logged-in user (`base.group_portal`/`base.group_user`) or the public
  (`base.group_public`) may create a report; a reporter may only read their
  own.
