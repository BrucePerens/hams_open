---
anchor: user_websites:COMM_appeal_action_reject
code_hash: sha256:b716f6d268578ccdfb2968c7a15761839925717cf82237eccac00bf957fb2682
---

# Claim: `ContentViolationAppeal.action_reject`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
checked this method against its sibling `action_approve` (reviewed separately) for the same two
questions: is there a server-side state guard, and does the message this method posts actually
match the target's real, current suspension status. Phrased in EARS, extended with the FOR-EACH
pattern.

FOR-EACH `appeal` IN `self` (a `content.violation.appeal` recordset, in whatever order Odoo
iterates it -- no ordering is imposed by this method itself):

1. THE system SHALL set `appeal.state` to `"rejected"`, unconditionally, regardless of `appeal`'s
   state at the time this method is called -- there is no read of `appeal.state` anywhere in this
   method's own body, so an appeal already in state `"rejected"` or already in state `"approved"`
   is rejected (or re-rejected) exactly the same way as one in state `"new"`.
2. THE system SHALL determine the message body from `appeal.group_id`'s truthiness alone: IF
   `appeal.group_id` is set, THEN THE system SHALL post `"Appeal rejected. The group remains
   suspended."`; IF NOT, THEN THE system SHALL post `"Appeal rejected. The user remains
   suspended."` -- neither branch reads `appeal.group_id.is_suspended_from_websites` or
   `appeal.user_id.is_suspended_from_websites` (the actual, separately-tracked suspension flags,
   `user_websites/models/user_websites_groups.py` and
   `user_websites/models/res_users_moderation.py`) to confirm the asserted "remains suspended"
   claim is still true at the moment the message is posted.
3. THE system SHALL post that message under service account
   `zero_sudo.mail_service_internal` (`appeal.with_user(mail_svc).message_post(...,
   subtype_xmlid="mail.mt_note")`), resolved once via `_get_service_uid` before the loop begins.
4. IF `_get_service_uid` cannot resolve `zero_sudo.mail_service_internal` to an active,
   correctly-flagged service account, THEN THE system SHALL raise `AccessError` (fail loudly, per
   `zero_sudo/models/security_utils.py`), before the loop over `self` begins, for every appeal in
   the batch equally.
5. THE system SHALL NOT call `action_pardon_user_websites()`, `action_pardon_group_websites()`, or
   any other method that reads or writes `is_suspended_from_websites` -- unlike `action_approve`,
   this method has no external side effect to reverse; the only persistent effects are the
   `state` write (statement 1) and the chatter message (statement 2-3).

**ISOLATION: THE system SHALL NOT let processing one appeal in `self` alter the state, message
content, or outcome computed for any other appeal in `self`** -- verified by reading: `mail_svc`
is resolved once, before the loop, from a fixed XML ID with no dependency on any appeal's own
data, so it is identical and read-only for every iteration; inside the loop, `appeal.state =
"rejected"` and the group/user branch of the message are each derived solely from that single
`appeal`'s own fields (no shared accumulator, no `.with_company()` context switch, no per-item
data read from a sibling record). This differs from `cron_notify_pending_reports`'s own FOR-EACH
(`user_websites/claims/cron_notify_pending_reports.md`), whose isolation failure was cross-item
data leakage and a cross-item failure dependency baked into the loop's own body; this method has
neither. The one caveat, noted for honesty rather than as a same-shape violation: if an ordinary
per-record `write()` raises for one appeal partway through the loop (e.g. an `ir.rule` denial on
an appeal outside the caller's `company_ids`, per `content_violation_appeal_multi_company_rule`),
Python's own `for` loop stops there and appeals later in iteration order are never reached in that
call -- but this is generic uncaught-exception behavior, not a cross-item data or state effect
this function's own body introduces, and (unlike the cron's periodically-committing context) an
ordinary object-button invocation runs inside a single request transaction with no explicit
`cr.commit()` in this method, so an unhandled exception here rolls the whole transaction back
rather than leaving a partially-applied batch of rejections.

## Findings against the sibling's own concern (state guard) and this method's own message text

**No server-side state guard exists**, matching `action_approve`. The only guard is client-side:
the form view's header buttons (`user_websites/views/content_violation_appeal_views.xml`) carry
`invisible="not id or state != 'new'"` on both `action_approve` and `action_reject`. That attribute
hides the button in the web client; it is not a security or ORM-level restriction, and does not
stop a direct method call (RPC, a server action, a stale browser tab that hasn't re-rendered after
another user's concurrent edit) from invoking `action_reject()` on a record already in state
`"approved"` or `"rejected"`. `access_content_violation_appeal_admin`/`_svc` both grant
`perm_write=1` with an unconditional `(1,'=',1)` `ir.rule` domain
(`content_violation_appeal_admin_rule`, `user_websites_security.xml`), so any administrator or the
service account can call this method on any appeal in any state.

Unlike `action_approve`, re-running this method on an already-processed appeal is **not** harmless
in every case:

- **Re-rejecting an already-rejected appeal** is idempotent in effect: `state` is written back to
  the same value, and a duplicate (but still accurate) "remains suspended" note is appended to the
  chatter. Mildly redundant, not misleading, assuming no intervening pardon.
- **Rejecting an appeal already in state `"approved"`** is a real, confirmed defect. Concrete
  scenario: Admin A opens an appeal (state `"new"`), clicks "Approve & Pardon" --
  `action_approve()` sets `state = "approved"` and calls `action_pardon_user_websites()`, which
  sets `is_suspended_from_websites = False` on the target user. Admin B has the same record open in
  a browser tab that has not refreshed since before A's action (still rendering the now-stale
  `"new"` state, so the "Reject Appeal" button is still visible in B's view) and clicks it. This
  method runs unconditionally: it overwrites `appeal.state` from `"approved"` back to `"rejected"`
  (the appeal's own audit trail now says "rejected," erasing the record that it was ever approved
  except via the chatter history) and posts "Appeal rejected. The user remains suspended." into
  that same chatter -- a statement that is **false** at the moment it is posted, since
  `is_suspended_from_websites` is already `False`. Per statement 2 above, this happens because the
  message is asserted from `appeal.group_id`'s static shape alone, never checked against the
  target's actual current suspension flag.
- **The same false-message outcome is reachable with a single admin and no race at all**, because
  nothing in this model prevents a user or group from accumulating more than one appeal record for
  the same suspension (`_check_appeal_target` only enforces exactly-one-of `user_id`/`group_id` per
  record, not one-appeal-per-target). If an earlier appeal for the same `user_id` was approved
  (pardoning them), and a later, separate `"new"` appeal by that same user is subsequently
  rejected -- entirely ordinary, sequential admin work, no stale tab required -- this method's
  message still unconditionally asserts "The user remains suspended," which is false the moment it
  posts.

**Downstream read-side effect of the resulting `state`/`is_suspended_from_websites` mismatch**:
grepped every read of `content.violation.appeal`'s `state` field in the codebase
(`user_websites/models/res_users.py` lines ~487 and ~636, both inside `_get_gdpr_streamed_keys()`
generators; the list/form views' badge/statusbar decorations). All of them are display-only --
they surface `state` as an informational "status" string in a GDPR data export or in the backend
UI. No enforcement code path was found that reads `content.violation.appeal.state` to decide
whether a user or group is actually blocked: the real enforcement checks
(`user_websites/models/content_violation_report.py`, `controllers/main.py`,
`user_websites/models/website_page.py`) all read `is_suspended_from_websites` directly on
`res.users`/`user.websites.group`, never this appeal's own `state`. So the mismatch above is a
data-integrity / audit-trail / user-facing-transparency defect (a GDPR export, or an admin later
reading the record, sees "rejected" for an appeal that was actually superseded by a pardon, and
the chatter itself carries a false "remains suspended" claim) -- not an access-control bypass,
since nothing downstream trusts this field for a real decision. This is a materially smaller
blast radius than `action_approve`'s own state-guard concern (which can call
`action_pardon_*_websites()` a second time), reviewed separately.

Classified against `hams_shared/agents/skills/bug-hunt/SKILL.md`'s known bug classes: none of the
17 listed classes is an exact fit. Closest is class 1 (vacuous/dead conditional branch) in reverse
shape -- here the *missing* branch (a state guard, or a live suspension-flag check before choosing
the message) is what's absent, not a present-but-unreachable one. Proposed as a candidate new bug
class in this pass's own report rather than added directly to the skill file, since a sibling pass
is reviewing `action_approve` in the same file concurrently and may independently propose the same
class from the mirror-image (double-approval) angle -- better to reconcile the wording once, after
both passes land, than have two near-duplicate entries race into the shared file.
