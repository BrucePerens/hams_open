# Story: A custom hostname Cloudflare would not let go

As a **System Administrator**,
I want a domain I delete in Odoo to really disappear from my Cloudflare zone,
so that removing a site does not quietly leave a live custom hostname behind on someone else's certificate and routing.

## Scenario: the delete fails, and the domain still goes

1. I delete an `edge.routing.domain` record. Odoo asks Cloudflare to delete the matching custom hostname first.

2. Cloudflare refuses -- a rate limit, a transient 5xx, a token that was revoked this morning. **The local record is still deleted.** Blocking it was considered and rejected: a permanently broken token would otherwise make the domain undeletable, which is a worse failure than the one it prevents.

3. A warning naming the domain, the hostname id and Cloudflare's own error goes into the log, exactly as before -- I should see the failure when it happens, not only later in a list.

4. Instead of that log line being the end of it, the orphaned hostname gets a retry record `[@ANCHOR: COMM_record_failed_hostname_delete]`. It holds the Cloudflare hostname id, the domain name, the website whose credentials and zone it belongs to, Cloudflare's error, and some counters. **It never holds an API token**; the token is fetched from the website again at retry time. A second failure for the same hostname reuses the same record rather than piling up rows for one remote object, and it reopens a record I had already closed -- a delete that just failed is evidence the hostname is still there.

## Scenario: it gets cleaned up without me

5. A background job retries every record whose wait has elapsed `[@ANCHOR: COMM_cron_retry_pending_hostname_deletes]`, handling each one independently `[@ANCHOR: COMM_pending_delete_attempt]` so that one bad record cannot take the rest of the run down with it.

6. Each retry asks Cloudflare once `[@ANCHOR: COMM_pending_delete_attempt_one]`. When Cloudflare confirms the delete -- **or says the hostname is not there at all**, because I removed it in the dashboard myself -- the record closes as Done. "Already gone" is success: the whole point was that this hostname must not exist.

7. When it fails again, the attempt is counted and the next try is pushed further out `[@ANCHOR: COMM_pending_delete_fail_attempt]`, on a backoff that grows and then stops growing `[@ANCHOR: COMM_pending_delete_next_attempt_at]` -- an unresolved record should keep checking occasionally, because a revoked token can be replaced, but it must never become a busy loop.

## Scenario: it needs me after all

8. After enough failures the record stops retrying by itself and is marked Abandoned. **It is not deleted** -- that would put me back where this story started, with a live hostname and nothing pointing at it.

9. I open Cloudflare Edge > Pending Hostname Deletes and see every one of them, newest trouble first, with the attempt count and Cloudflare's last error `[@ANCHOR: COMM_pending_delete_compute_name]`.

10. Having fixed the token, I press **Retry Now** `[@ANCHOR: COMM_pending_delete_action_retry_now]` and it goes through -- retrying works on an abandoned record, which is exactly the case abandoning exists to wait for. Or I press **Abandon** `[@ANCHOR: COMM_pending_delete_action_abandon]` on one I have dealt with by hand, to stop it being retried while keeping it on the list.

11. Both buttons are mine alone: they make a real, credentialed Cloudflare call, so they check for real admin rights before reaching the network `[@ANCHOR: COMM_pending_delete_check_admin]`, and no ACL row grants this model to an ordinary user at all.

**Status:** Verified by `[@ANCHOR: COMM_test_pending_delete_views_render]` and the rest of `cloudflare/tests/test_hostname_pending_delete.py`.

**Decision:** Bruce, 2026-09-19 -- "3 sounds good to me", the third option on `hams_com/night_shift_questions/answered/cloudflare-hostname-delete-failure-block-or-proceed-05a5144b.md` ("Proceed, but keep a retry record").
