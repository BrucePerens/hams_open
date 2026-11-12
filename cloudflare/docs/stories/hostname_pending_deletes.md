# Story: A custom hostname Cloudflare would not let go

As a **System Administrator**,
I want a domain I delete in Odoo to really disappear from my Cloudflare zone,
so that removing a site does not quietly leave a live custom hostname behind on someone else's certificate and routing.

The model is `cloudflare.hostname.pending.delete` (`models/hostname_pending_delete.py`). One row is one custom hostname Cloudflare refused to delete. Fields: `cf_hostname_id` (Char, required, indexed; Cloudflare's id, the key of the row), `domain_name` (Char, required), `website_id` (Many2one to `website`, set null on delete; the website whose credentials and zone apply, a pointer only), `last_error` (Char, Cloudflare's own message cut to 500 characters), `attempt_count` (Integer, default 0), `last_attempt_at` and `next_attempt_at` (Datetime, the latter indexed and false once nothing is due), `state` (`pending`, `done` or `abandoned`, default `pending`), and a computed `name`. All are read-only in the views; rows are ordered by state, next attempt, id. No token is ever stored. Constants: `MAX_DELETE_ATTEMPTS = 8` and `BACKOFF_MINUTES = [5, 10, 20, 40, 80, 160, 320, 720]`. Access: Settings administrators and the tunnel service account have full rights; no ordinary user has any.

## Scenario: the delete fails, and the domain still goes

1. I delete an `edge.routing.domain` record. Odoo asks Cloudflare to delete the matching custom hostname first.

2. Cloudflare refuses -- a rate limit, a transient 5xx, a token that was revoked this morning. **The local record is still deleted.** Blocking it was considered and rejected: a permanently broken token would otherwise make the domain undeletable, which is a worse failure than the one it prevents.

3. A warning naming the domain, the hostname id and Cloudflare's own error goes into the log, exactly as before -- I should see the failure when it happens, not only later in a list.

4. Instead of that log line being the end of it, the orphaned hostname gets a retry record. It holds the Cloudflare hostname id, the domain name, the website whose credentials and zone it belongs to, Cloudflare's error, and some counters. **It never holds an API token**; the token is fetched from the website again at retry time. A second failure for the same hostname reuses the same record rather than piling up rows for one remote object, and it reopens a record I had already closed -- a delete that just failed is evidence the hostname is still there.

### `_record_failed_delete(domain_name, cf_hostname_id, website, error)` [@ANCHOR: COMM_record_failed_hostname_delete]

Called by `edge.routing.domain.unlink()` (in `models/domain.py`) after Cloudflare's delete failed. `domain_name` is the domain string, `cf_hostname_id` Cloudflare's custom hostname id, `website` the website record whose credentials were used (or empty), `error` Cloudflare's message. It returns the pending-delete record, or an empty recordset when `cf_hostname_id` is empty. The row is keyed on `cf_hostname_id`: an existing row in any state, including `done` or `abandoned`, is rewritten to `pending` with the new error and a due time computed from its current attempt count, so one remote object never has two rows; otherwise a row with attempt count 0 is created due in 5 minutes. The error is cut to 500 characters and is Cloudflare's own text, so it cannot carry a token. It does not use `sudo`: it runs with the caller's rights (an administrator in `unlink`, the cron's service account on a retry). No network call. Tests: `test_01`, `test_03_a_second_failure_for_the_same_hostname_reuses_one_record` and `test_04_a_failure_reopens_a_record_that_was_already_closed` in `tests/test_hostname_pending_delete.py`.

## Scenario: it gets cleaned up without me

5. A background job retries every record whose wait has elapsed, handling each one independently so that one bad record cannot take the rest of the run down with it.

### Retry cron [@ANCHOR: COMM_cron_retry_pending_hostname_deletes]

`_cron_retry_pending_deletes()` on `cloudflare.hostname.pending.delete`, run by the scheduled action "Cloudflare: Retry Pending Custom Hostname Deletes" every 5 minutes as the tunnel service user. It searches up to 1000 records in state `pending` whose `next_attempt_at` is empty or in the past, hands them to `_attempt_delete`, logs one summary line and returns the summary dict `{"done", "retrying", "abandoned", "skipped"}` (all zero when nothing was due). `abandoned` and `done` records are never picked up. It makes one Cloudflare call per due record, blocking, within the cron's own transaction. Test: `test_05_the_cron_closes_a_record_when_cloudflare_confirms` in `tests/test_hostname_pending_delete.py`.

### `_attempt_delete()` [@ANCHOR: COMM_pending_delete_attempt]

Tries the Cloudflare delete once for each record in `self` and returns the `{outcome: count}` summary. Each record runs inside its own database savepoint, which matters because a failed query otherwise leaves the whole transaction unusable and every later record would fail with it. An unexpected exception from one record is logged with its hostname id, counted under `retrying`, and the loop continues; the record stays `pending`. It takes no arguments, makes network calls through `_attempt_one_delete`, and is used by the cron and by the "Retry Now" button.

6. Each retry asks Cloudflare once. When Cloudflare confirms the delete -- **or says the hostname is not there at all**, because I removed it in the dashboard myself -- the record closes as Done. "Already gone" is success: the whole point was that this hostname must not exist.

### `_attempt_one_delete()` [@ANCHOR: COMM_pending_delete_attempt_one]

One record (`ensure_one`), at most one Cloudflare call, returns one of `"done"`, `"retrying"` or `"abandoned"`. With no website (it was deleted) or no token or zone id for it, it books a failed attempt without calling Cloudflare. Otherwise it calls `delete_custom_hostname(cf_hostname_id, token, zone_id)`, which reports a 404 as success. On success it writes state `done`, increments `attempt_count`, stamps `last_attempt_at`, and clears `next_attempt_at` and `last_error`. On failure it books the failure through `_fail_attempt` and returns `"abandoned"` if that reached the cap, else `"retrying"`. The token is read from the website each time and not logged. Tests: `test_05_the_cron_closes_a_record_when_cloudflare_confirms`, `test_06_a_hostname_that_is_already_gone_counts_as_done` and `test_07_repeated_failure_counts_up_and_eventually_stops` in `tests/test_hostname_pending_delete.py`.

7. When it fails again, the attempt is counted and the next try is pushed further out, on a backoff that grows and then stops growing -- an unresolved record should keep checking occasionally, because a revoked token can be replaced, but it must never become a busy loop.

### `_fail_attempt(message)` [@ANCHOR: COMM_pending_delete_fail_attempt]

Books one more failed attempt on one record: it increments `attempt_count`, stamps `last_attempt_at` and stores `message` (cut to 500 characters) in `last_error`. If the count reaches `MAX_DELETE_ATTEMPTS` (8), the state becomes `abandoned`, the next attempt is cleared and a warning says the hostname may still be active and an administrator can retry it from Cloudflare Edge > Pending Hostname Deletes. Otherwise the state stays `pending` and the next attempt is set from the backoff. Abandoning does not delete the row, because the orphaned hostname must stay visible. Returns nothing. Test: `test_07_repeated_failure_counts_up_and_eventually_stops`.

### `_next_attempt_at(attempts_made, now=None)` [@ANCHOR: COMM_pending_delete_next_attempt_at]

Returns the datetime when the retry after `attempts_made` failures becomes due: `now` plus the entry of `BACKOFF_MINUTES` (5, 10, 20, 40, 80, 160, 320, then 720 minutes) at index `attempts_made`, clamped to the table at both ends, so a count outside it still gives a valid time and never raises inside the cron. `now` is optional and exists so a test can give several calls one base time and compare them exactly; by default it is the current time. `@api.model`, pure apart from reading the clock. With eight attempts the backoff spans about a day and a half. Tests: `test_01_a_failed_cloudflare_delete_creates_a_pending_record` (the first due time) and `test_08_the_backoff_grows_and_is_capped`.

## Scenario: it needs me after all

8. After enough failures the record stops retrying by itself and is marked Abandoned. **It is not deleted** -- that would put me back where this story started, with a live hostname and nothing pointing at it.

9. I open Cloudflare Edge > Pending Hostname Deletes and see every one of them, newest trouble first, with the attempt count and Cloudflare's last error.

### Display name [@ANCHOR: COMM_pending_delete_compute_name]

`_compute_name()` computes the read-only `name` field of each record (`@api.depends("domain_name", "cf_hostname_id")`) as `"<domain> (<hostname id>)"`, using "Unknown domain" and `-` when a part is missing. It is what the list and form views and the breadcrumb show. Pure, no side effects. The test that cites it, `test_11_pending_delete_views_render`, builds the list and form views for the model.

10. Having fixed the token, I press **Retry Now** and it goes through -- retrying works on an abandoned record, which is exactly the case abandoning exists to wait for. Or I press **Abandon** on one I have dealt with by hand, to stop it being retried while keeping it on the list.

### Retry Now button [@ANCHOR: COMM_pending_delete_action_retry_now]

`action_retry_now()`, `@public_rpc`, on the selected records. It first runs `_check_pending_delete_admin` (so a caller without rights is refused before any network call), then `_attempt_delete()` on the selected records at once, whatever their state and due time (an `abandoned` record can be retried; that is why abandoning keeps the row). It returns `True`. The outcome shows in the record's state, attempts and last error after the view reloads. Blocking Cloudflare calls, one per record. Test: `test_09_an_admin_can_retry_a_record_by_hand` in `tests/test_hostname_pending_delete.py`.

### Abandon button [@ANCHOR: COMM_pending_delete_action_abandon]

`action_mark_abandoned()`, `@public_rpc`. After `_check_pending_delete_admin` it writes `state = "abandoned"` and clears `next_attempt_at` on the selected records, so the cron stops retrying them while they stay on the list; the attempt count and last error are kept. Returns `True`. No network call. It is for a hostname the administrator removed by hand. Test: `test_09_an_admin_can_retry_a_record_by_hand`, which also presses Abandon.

11. Both buttons are mine alone: they make a real, credentialed Cloudflare call, so they check for real admin rights before reaching the network, and no ACL row grants this model to an ordinary user at all.

### Admin check [@ANCHOR: COMM_pending_delete_check_admin]

`_check_pending_delete_admin()` raises `AccessError` ("You are not authorized to manage pending Cloudflare hostname deletes.") unless the current user is a Settings administrator (`base.group_system`) or a service account. It is called first by both buttons: they are public methods, reachable by `/web/dataset/call_kw` for anyone who can name a record id, and Retry Now makes a real credentialed call, so the check is explicit instead of relying on the ACL of the first field read. It takes no arguments, returns nothing and has no side effects. Test: `test_10_a_non_admin_can_neither_read_nor_drive_the_model`, which asserts both buttons raise `AccessError` and that the Cloudflare delete function is never called.

### Views test [@ANCHOR: COMM_test_pending_delete_views_render]

`test_11_pending_delete_views_render` asks for the compiled list and form views of `cloudflare.hostname.pending.delete`. The two views are marked in `views/hostname_pending_delete_views.xml` as tested by this anchor rather than by a browser tour, because their buttons make real Cloudflare calls. It proves that the views build against the model's fields, including the computed `name`. The rest of `cloudflare/tests/test_hostname_pending_delete.py` covers the model's behaviour.

**Status:** Verified by the views test above and the rest of `cloudflare/tests/test_hostname_pending_delete.py`.

**Decision:** Bruce, 2026-09-19 -- "3 sounds good to me", the third option on `hams_com/night_shift_questions/answered/cloudflare-hostname-delete-failure-block-or-proceed-05a5144b.md` ("Proceed, but keep a retry record").
