# Story: Public Forms Are Protected From Programs Without a Third-Party Captcha

As the **Site Owner**, I want **the forms that anyone can send (report a violation, request an invitation, contact us, newsletter, event
registration) to turn away programs without a Google or Cloudflare captcha**, so that my moderators read people's reports and my visitors are
not tracked by a third party to prove they are people.

## Scenario: A visitor reports a page

The Report Violation button opens a modal on a page that may come from Cloudflare's edge cache. As the modal opens, the browser asks
`/form_shield/challenge` for a challenge of its own and solves it in a Web Worker, telling the visitor "Verifying you are human…" in a polite live region. Typing is never blocked; pressing Send early waits for the answer and sends.

The functions behind those steps each carry their own anchor. The proof-of-work solver:. The browser session:. The form wiring:. The refusal wording shown after a redirect:.

## Scenario: The server decides

The challenge must be signed, unexpired, issued to the same /24 (IPv4, including an IPv4-mapped IPv6 address) or /48 (IPv6) network, at least three seconds old, solved for this very form and
never used before; then the per-network, per-page and per-form limits are counted. A filled honeypot or a bad solution is answered like a success and stores nothing;
an expired challenge, a changed network or a wrong answer tells the visitor to try again; a rate limit answers 429; Redis being down answers 503.
A guest must give a valid address and say at least ten characters that are not a single random-looking string, link to the site's own pages, and
use at most two links.

## Scenario: A visitor cannot run the check

A browser with scripts off (the modal cannot open at all) or no Web Worker is offered a page with one question in words, new for each visit and
readable by a screen reader.

## Scenario: A signed-in member reports

A member skips the challenge and meets the honeypots and a looser limit, counted per user.

## Scenario: Odoo's own forms

The website form builder (Contact Us), event registration, newsletter and follow ask the stock `ReCaptcha` class for a token. `form_shield` makes
that token its own answer, loads no Google script, and checks it on the server where Odoo calls its captcha hook.

## Scenario: The administrator sees what is being turned away

Settings, Form shield counters shows how many submissions per day each form accepted and refused, by reason. It keeps no address, email or text, and
drops rows after 90 days.

## Scenario: The pieces, for a developer

A form puts `form_shield.fields` inside a `<form data-form-shield="ID">`. A controller calls
`check_form`, which reads the standard fields and runs the checks; a refusal the
controller decides itself is counted the same way. A form that redirects hands the verdict to
`refusal_response`, which sends a program to the success page, a person back with
`?form_shield_error=` (shown by) and a rate limit or a Redis failure to a page with 429 or 503. Challenges are issued by. The signing key is seeded once at install, and the daily counters are purged by a cron.

## Scenario: A visitor reports a listing, a repeater, an event or a forum post

Those forms (hams.com's `ham_classifieds`, `ham_repeater_dir`, `ham_events`, `ham_forum_extension`) and the request-an-invite page use the same shield
with their own form ids, their own per-target limits, and, on pages that can hold it, the question variant at `?q=1`.

## Scenario: Internals a maintainer can rely on

The signing key and tunables are read through one helper each; the visitor's network
comes from the trusted-client-address utility, never from a header the visitor can forge. A challenge is built and
signed by with; the network is stored only as a keyed hash;
the parsed challenge is a small value object; an answer is `challenge~solution`.
The proof of work hashes `challenge:form:counter`, counts leading zero bits, and the Python
twin of the browser's solver is. The question variant derives its two numbers from the signed nonce. A guest's answer is checked in one place; a
challenge may be used once; the limits count in Redis under per-database keys on the shared connection; every refusal is counted by reason and the counters show a readable name. Redirecting forms build their paths with.

## Reference

Each entry below is one anchor, read from the code in `form_shield/` and `user_websites/controllers/main.py`. Signatures are given as the code has them. Unless an entry says otherwise a function is synchronous, pure, raises nothing of its own, and touches no database, Redis or transmitter. The module keeps no address, email or typed text anywhere; the only state is Redis (one-use markers and rate counters, every key expiring on its own) and the per-day counters table. Tests are in `form_shield/tests/` and `form_shield/static/tests/`.

### Challenge and proof of work, server side (`tokens.py`)

The wire format of a challenge is `v1.<kind>.<issued>.<expires>.<difficulty>.<nonce>.<ip_tag>.<signature>`: eight dot-separated ASCII fields, `kind` being `pow` or `q`, times in whole seconds, `nonce` 16 hex characters, `ip_tag` 16 hex characters and `signature` the first 40 hex characters of an HMAC-SHA-256 over everything before it. A visitor's answer is `<challenge>~<solution>` (at most 400 characters). The proof of work is SHA-256 over the UTF-8 bytes of `<challenge>:<form_id>:<counter>`, the counter in decimal from 0; a solution is the smallest counter whose digest starts with `difficulty` zero bits.

### `_mac(key, text)` [@ANCHOR: form_shield:mac]

`_mac(key: str, text: str) -> str`. The lowercase hex HMAC-SHA-256 of `text` under `key`, both encoded as UTF-8. Used to sign a challenge and to build the keyed network hash. Pure; cannot fail for strings. Test: `test_tampered_challenge_is_refused`.

### `ip_prefix(address)` [@ANCHOR: form_shield:ip_tag]

`ip_prefix(address: str | None) -> str`. The /24 network of an IPv4 address or the /48 network of an IPv6 address, as text such as `203.0.113.0/24`; an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) is reduced to its IPv4 /24, because otherwise every such address would share one /48. An unparseable, empty or missing address gives `"none"`. A challenge is bound to the network rather than the exact address so a phone moving between towers keeps it. Pure. Test: `test_ip_tag_uses_prefix_and_keeps_no_address`.

### `ip_tag(key, address)` [@ANCHOR: form_shield:ip_tag_hash]

`ip_tag(key: str, address: str) -> str`. A 16-character keyed hash of `"ip|" + ip_prefix(address)`: the first 16 hex characters of `_mac`. It is what goes into a challenge and what rate limits count, so no address is ever stored; without the key the network cannot be recovered. Pure. Test: `test_ip_tag_uses_prefix_and_keeps_no_address`.

### `make_challenge(key, address, now, difficulty, kind, ttl)` [@ANCHOR: form_shield:make_challenge]

`-> str`. Builds and signs a challenge issued at `now` (seconds, truncated to an int) that expires at `now + ttl`, with the given `difficulty` (leading zero bits), `kind` (`pow` or `q`), a fresh random 8-byte hex nonce and the `ip_tag` of `address`. Costs a few microseconds and touches no state, so any worker can issue and any other can check it. `key` is the signing key; raises nothing for valid inputs. Test: `test_tampered_challenge_is_refused`.

### `Parsed` [@ANCHOR: form_shield:parsed_challenge]

`Parsed(token, kind, issued, expires, difficulty, nonce, tag)`, a small value object returned by `parse_challenge` for a challenge whose signature held. Fields: `token` the whole challenge string, `kind` `"pow"` or `"q"`, `issued` and `expires` integer seconds, `difficulty` an integer, `nonce` the 16-character hex string, `tag` the network hash. It is immutable by convention (nothing writes to it) and lives for one request. Test: `test_tampered_challenge_is_refused`.

### `split_response(response)` [@ANCHOR: form_shield:split_response]

`-> (challenge, solution)`. Splits a visitor's answer `<challenge>~<solution>` at the first `~`, stripping whitespace. Returns `(None, None)` when the text is empty, has no separator, has an empty challenge or is longer than 400 characters; an empty solution is allowed through here and refused later. Pure. Test: `test_check_refusal_table`.

### `parse_challenge(key, token)` [@ANCHOR: form_shield:parse_challenge]

`-> (Parsed, None) | (None, reason)`. Checks a challenge's shape and signature. The reason is `bad_format` for anything that is not eight dot-separated fields starting with `v1` or whose times or difficulty are not integers, and `bad_signature` when the HMAC (keyed-hash message authentication code) does not match; the signature is compared with `hmac.compare_digest` on bytes, so a non-ASCII visitor-supplied signature cannot raise. It does not check expiry, network or use; `form.shield._check_response` does. Pure. Test: `test_tampered_challenge_is_refused`.

### `leading_zero_bits(digest)` [@ANCHOR: form_shield:leading_zero_bits] The value for an all-zero 32-byte digest is 256; the value for a digest whose first byte is 0x00 and second 0x80 is 8. `pow_valid` compares it with the challenge's difficulty. Argument `digest` is raw bytes, not hex; an empty bytes object gives 0.

`leading_zero_bits(digest: bytes) -> int`. How many zero bits the digest starts with (`8 * len(digest)` for all zeros). Pure. Python twin of the browser's `leadingZeroBits`. Test: `test_pow_vector_matches_the_browser_solver`.

### `pow_digest(token, form_id, counter)` [@ANCHOR: form_shield:pow_digest]

`-> bytes`. SHA-256 of `f"{token}:{form_id}:{counter}"` encoded as UTF-8: the proof-of-work hash. Binding `form_id` into the hash ties a solution to one form without the form being known at issue time. Pure; `counter` is any value that formats as decimal. Test: `test_pow_vector_matches_the_browser_solver`, which checks the same vector as the browser's solver.

### `pow_valid(token, form_id, counter, difficulty)` [@ANCHOR: form_shield:pow_valid]

`-> bool`. True when `counter` is a canonical decimal (ASCII digits only, at most 12 characters, no leading zeros, so each counter has exactly one spelling) and the digest has at least `difficulty` leading zero bits. Any other input, including a non-string that does not format as digits, gives False; it never raises. Pure and cheap: one hash. Test: `test_pow_vector_matches_the_browser_solver`.

### `solve_pow(token, form_id, difficulty, start=0)` [@ANCHOR: form_shield:solve_pow]

`-> int`. The smallest counter at or above `start` that solves the challenge: a plain loop over `pow_digest`. It is the Python twin of the browser's solver, used by tests, and blocks until found (about `2**difficulty` hashes on average, so never call it with a high difficulty on a request thread). Test: `test_pow_vector_matches_the_browser_solver`.

### `question_numbers(nonce)` [@ANCHOR: form_shield:question_numbers]

`-> (first, second)`. The two numbers of the question fallback, each from two hex characters of the signed nonce: `first = 2 + int(nonce[0:2], 16) % 9`, `second = 2 + int(nonce[2:4], 16) % 9`, so each is 2 to 10. Deterministic, so the server needs no state to know the answer. Raises `ValueError` for a nonce that is not hex in those positions (callers catch it). Test: `test_question_fallback_accepts_the_right_answer`.

### `question_text(nonce)` [@ANCHOR: form_shield:question_text]

`-> str`. The question in words, for example "What is three plus seven? Type your answer as digits.", built from `question_numbers` and the number-word table. Plain text so a screen reader reads it; the same nonce always gives the same words. Test: `test_question_fallback_accepts_the_right_answer`.

### `question_valid(nonce, answer)` [@ANCHOR: form_shield:question_valid] The comparison is exact string equality after stripping whitespace, so `"10 "` is accepted and `"ten"` or `"+10"` is not. The caller `_check_response` passes the solution half of the visitor's answer and counts a wrong one as `wrong_answer`, outcome `retry`. A nonce too short or not hex is treated as a refusal, not an error.

`-> bool`. True when the stripped `answer` equals `str(first + second)`; an empty answer or a bad nonce gives False and never raises. Pure. Test: `test_question_fallback_accepts_the_right_answer`.

### The shield model (`models/form_shield.py`, `form.shield`)

`form.shield` is an abstract model; a form calls `env["form.shield"].check(...)` (or `check_form`) once per submission and acts on the returned `Verdict(ok, outcome, reason)` namedtuple. `outcome` is one of `ok`, `silent` (a program: answer like a success and store nothing), `retry` (a person: ask them to try again), `question` (no answer at all: offer the question), `limited` (rate limit, HTTP 429) or `unavailable` (Redis down, HTTP 503). Tunables are `ir.config_parameter` keys: `form_shield.difficulty` (default 19, capped at 28), `form_shield.ttl_seconds` (7200), `form_shield.min_age_seconds` (3), `form_shield.question_min_age_seconds` (6), `form_shield.retention_days` (90).

### `_int_param(name, default)` [@ANCHOR: form_shield:int_param]

`-> int`. Reads the system parameter `name` and returns it as an int, or `default` when unset or empty; a value that is not a number is logged at error level and replaced by `default`, so a bad setting never breaks a form. `@api.model`; one parameter read. Test: `test_check_refusal_table`.

### `_signing_key()` [@ANCHOR: form_shield:signing_key]

`-> str`. The signing key from `form_shield.signing_key`, a 64-hex-character secret seeded once at install by `post_init_hook`. Raises `RuntimeError("form_shield.signing_key is not set")` when it is missing, so a broken install fails loudly rather than signing with an empty key. The key is never logged or sent to a browser; replacing it only invalidates challenges in flight. Test: `test_post_init_hook_seeds_key_once`.

### `_client_address()` [@ANCHOR: form_shield:client_address] It is called when a challenge is issued and again when an answer is checked, and the two network hashes must match or the answer is refused as `ip_changed`. The function is `@api.model`, takes no arguments, reads no database rows and writes nothing; the utility's own failure modes are documented with `zero_sudo`.

`-> str`. The visitor's address from `zero_sudo.security.utils._get_trusted_client_ip()`, which takes it from the trusted proxy chain and never from a header the visitor can forge. Used only to compute the network hash. Test: `test_challenge_endpoint_issues_signed_uncacheable_json`.

### `issue(kind="pow")` [@ANCHOR: form_shield:issue]

`-> dict`. A new challenge for the current visitor, the dict the challenge route returns as JSON: `kind`, `challenge`, `difficulty`, `min_age` (seconds the answer must wait), `expires_in` (the TTL: time to live) and, for kind `q`, `question` (the words). For a proof of work the difficulty is `form_shield.difficulty` clamped to 0 to 28; for a question it is 0. `@api.private` (not callable over RPC). Reads parameters and the key, writes nothing. Test: `test_challenge_endpoint_issues_signed_uncacheable_json`.

### `_redis()` [@ANCHOR: form_shield:redis_connection] `@api.model`, no arguments. It is created per call (the pool is shared), so callers such as `_claim_nonce` and `_over_limit` simply call it. A Redis outage surfaces as `redis.exceptions.RedisError` or `OSError` on the first command, never from this function itself, and `check` answers `unavailable` rather than skipping the check.

`-> redis.Redis`. The shared connection from `distributed_redis_cache.redis_pool.get_redis_connection(env)`. Redis errors raised by later calls are caught in `check`, which answers `unavailable`. Test: `test_check_accepts_a_solved_challenge_once`.

### `_key(*parts)` [@ANCHOR: form_shield:redis_key] Example: `form_shield:hams_prod:nonce:3f9a...` for a one-use marker and `form_shield:hams_prod:limit:report_violation:subject:net:ab12...` for a counter. `@api.model`; pure apart from reading the database name; no argument is validated, so callers pass only their own constants and hashes.

`-> str`. A Redis key `form_shield:<database name>:<parts joined by colons>`, so databases sharing one Redis never share counters. It never contains an address, only the network hash or a user id. Test: `test_counters_keep_no_personal_data`.

### `_reject(form_id, reason, outcome)` [@ANCHOR: form_shield:reject]

`-> Verdict(False, outcome, reason)`. Logs the refusal at info level and counts it in today's `form.shield.rejection` row (on a cursor of its own when the context has `form_shield_own_cursor`, because a caller that is about to raise an error rolls the request's transaction back). Writes one counter, keeps no text. Test: `test_check_refusal_table`.

### `refuse(form_id, reason, outcome=RETRY)` [@ANCHOR: form_shield:refuse]

`-> Verdict`. Counts a refusal that the calling controller decided on itself (a field checked after the shield passed) under the same per-day counters and returns the verdict. `@api.private`. Test: `test_report_problem_reasons` in `user_websites`.

### `check(form_id, response, honeypots=(), target=None, limits=None, own_cursor=False)` [@ANCHOR: form_shield:check]

`-> Verdict`. Decides whether a submission may proceed.

| Argument | Meaning |
|---|---|
| `form_id` | the form's name, used in the hash binding, the counters and the rate-limit keys |
| `response` | the visitor's `<challenge>~<solution>`; ignored for a signed-in member |
| `honeypots` | values of fields a person never sees; any text in one is a program (`silent`) |
| `target` | what the form is about (a page, a listing), so one target cannot be flooded from many networks |
| `limits` | table of `(scope, allowed, window seconds)`, default `DEFAULT_LIMITS` (6 per network per hour, 20 per target per day, 300 per form per hour) or, for a member, `MEMBER_LIMITS` (20 per user per hour) |
| `own_cursor` | write the counter on a separate cursor |

Order: honeypots; for a guest `_check_response`; the rate limits; then it counts `accepted`. Redis or socket errors are logged and answer `unavailable`, never "ok": a shield that switches itself off when its store is down is what a spammer waits for. A signed-in member skips the challenge (member pages are never edge-cached, and a member is accountable) but meets the honeypots and the limits, counted per user. `@api.private`. Tests: `test_check_accepts_a_solved_challenge_once`, `test_check_refusal_table`.

### `check_form(form_id, post, honeypots=(), target=None, limits=None)` [@ANCHOR: form_shield:check_form]

`-> Verdict`. `check()` for a submitted HTML form: it reads `shield_response` from `post` (a dict of posted values), or, when that is empty and `shield_challenge` is present, builds `<shield_challenge>~<shield_answer>` for the question form, and adds the shield's own honeypot `fs_homepage` to `honeypots`. `@api.private`. Test: `test_check_form_reads_the_standard_fields`.

### `_check_response(form_id, response)` [@ANCHOR: form_shield:check_response]

`-> Verdict | None`. None when a guest's answer is good, else the refusing verdict, tested in this order: no parseable answer gives `no_token` (`question`); `bad_format` or `bad_signature` (`silent`); past `expires` gives `expired` (`retry`); another network gives `ip_changed` (`retry`); younger than the minimum age gives `too_fast` (`retry`); a wrong question answer gives `wrong_answer` (`retry`); a bad proof gives `bad_solution` (`silent`); a challenge already used gives `replay` (`silent`). Only a challenge that passed everything is consumed (`_claim_nonce`). Test: `test_check_refusal_table`.

### `_claim_nonce(parsed, now)` [@ANCHOR: form_shield:claim_nonce]

`-> bool`. True the first time this challenge's nonce is presented, False after: a Redis `SET key 1 NX EX` where the expiry is the challenge's remaining life plus 60 seconds, so markers clean themselves up. The one piece of server state the stateless challenges need; it may raise a Redis error, which `check` turns into `unavailable`. Test: `test_check_accepts_a_solved_challenge_once`.

### `_over_limit(form_id, subject, target, table)` [@ANCHOR: form_shield:over_limit]

`-> str | None`. The name of the first exceeded limit (`rate_subject`, `rate_target` or `rate_global`), else None. For each scope in order it increments a Redis counter (the target is hashed first), sets its expiry to the window on first use, and stops at the first scope over its allowance, so a visitor already over their own limit stops eating the budgets other people share. Redis errors propagate to `check`. Test: `test_check_refusal_table`.

### `FormShieldRejection` counters (`models/form_shield_rejection.py`)

The model `form.shield.rejection` holds one row per day, form and reason: fields `day` (date), `form_id` (char, at most 64 stored), `reason` (char), `count` (integer), a computed `name`, with a unique constraint on `(day, form_id, reason)`. It holds no address, email or text. Only the service account writes it.

### `_compute_name()` [@ANCHOR: form_shield:rejection_name] The name is a stored-nowhere computed field (no `store=True`), recomputed when shown, so it costs nothing in the table and cannot leak anything the three fields do not already hold. It takes the recordset `self`, loops over it and sets `name` on each row.

Computes each row's display name as `"<day> <form_id> <reason>"`, so the Settings list shows a readable name. `@api.depends` on those three fields; no side effects. Test: `test_counters_keep_no_personal_data`.

### `_count(form_id, reason, own_cursor=False)` [@ANCHOR: form_shield:count]

Adds one to today's counter for `(form_id, reason)` as the service account `form_shield.user_form_shield_service` (never the public user). In the caller's transaction by default; with `own_cursor` on a new cursor of the registry so an error raised afterwards cannot roll the count back. Test: `test_counters_keep_no_personal_data`.

### `_upsert(form_id, reason)` [@ANCHOR: form_shield:count_upsert]

One parameterised `INSERT ... ON CONFLICT (day, form_id, reason) DO UPDATE SET count = count + 1` into `form_shield_rejection` with today's date, `form_id` and `reason` cut to 64 characters; flushes the model first and invalidates its cache after. Atomic under concurrency, so no counts are lost. Test: `test_counters_keep_no_personal_data`; the day is UTC whatever the context time zone: `test_counter_day_is_utc_whatever_the_context_time_zone`.

### `_cron_purge()` [@ANCHOR: form_shield:purge] `@api.model`, no arguments, returns an int. It searches in one query with `day < today - retention_days` and unlinks the rows, so counters older than the retention window disappear and the table stays small; a retention parameter that is not a number falls back to 90 with an error log. It runs as the service account, whose group may delete this model.

Deletes counter rows older than `form_shield.retention_days` (default 90), at most 100000 per run, and returns the number deleted. Run daily by the cron below as the service account. Test: `test_purge_drops_old_counters`.

### Service account and access [@ANCHOR: form_shield:service_account_acl]

`security/form_shield_security.xml` creates the service user `form_shield_service_internal` (`form_shield.user_form_shield_service`, a service account) in the group `form_shield.group_form_shield_service`; the shield writes its daily counters as this account only, and the anonymous public user has no access to the counters model. Test: `test_counters_keep_no_personal_data`.

### Purge cron [@ANCHOR: form_shield:cron_purge] Interval one day. Because the code is `model._cron_purge()` and the cron's user is the service account, the delete is checked against that account's access rights; no superuser is involved. The record is in a normal data block, so edits to it reach upgraded databases.

The `ir.cron` record `ir_cron_form_shield_purge`: daily, runs `model._cron_purge()` as `form_shield.user_form_shield_service`, active on install. Test: `test_purge_drops_old_counters`.

### `post_init_hook(env)` [@ANCHOR: form_shield:post_init_hook]

Seeds `form_shield.signing_key` with `secrets.token_hex(32)` when it is not set (an existing key is never replaced, which would invalidate challenges in flight), then bootstraps the module's documentation article as the Cloudflare WAF service account (`cloudflare.user_cloudflare_waf`) through `ir.module.module._bootstrap_knowledge_docs()`. Runs once at install; an error from the service-account lookup or the bootstrap fails the install. Test: `test_post_init_hook_seeds_key_once`.

### Challenge route (`controllers/form_shield_api.py`) [@ANCHOR: form_shield:challenge_route]

`FormShieldController.challenge(self, kind="pow", **_ignored)`, route `POST /form_shield/challenge`, `auth="public"`, `csrf=False`, `readonly=True`, `save_session=False`, no sitemap entry. `kind="q"` asks for a question challenge, anything else a proof of work. Returns the JSON of `form.shield.issue(kind)` with `Cache-Control: no-store`. A POST rather than a GET because pages that call it may come from Cloudflare's edge cache and every visitor needs a challenge of their own (network-bound, single-use). It sets no cookie and writes nothing, so it needs no session or CSRF (cross-site request forgery) token: all it can do is hand out a signed string. Test: `test_challenge_endpoint_issues_signed_uncacheable_json`.

### Refusal helpers (`refusal.py`)

### `_with_query(path, query)` [@ANCHOR: form_shield:with_query] Example: `_with_query('/page', 'a=1')` gives `/page?a=1` and `_with_query('/page?x=2', 'a=1')` gives `/page?x=2&a=1`. It does no escaping, so the query must already be safe; `refusal_response` passes `form_shield_error=` plus a reason from a fixed list.

`-> str`. Appends `query` to `path` with `?`, or with `&` when the path already has a query string. Pure; callers pass literals or paths they built, never visitor input. Test: `test_refusal_response_by_outcome`.

### `refusal_response(verdict, back_path, success_path, question_path=None)` [@ANCHOR: form_shield:refusal_response]

For a form that redirects: turns a refused `Verdict` into the response. `silent` redirects to `success_path`, exactly where a real success goes, so a program learns nothing. `limited` (429) and `unavailable` (503) render the `form_shield.refused_page` template with that status and `Cache-Control: no-store`. `question` redirects to `question_path` when the form has a page for the question. Everything else redirects to `back_path` with `form_shield_error=<reason>`, which the page script turns into a notification. All three paths must be local (start with `/`); the redirects are made with `local=True`. Test: `test_refusal_response_by_outcome`.

### Odoo's own forms (`models/ir_http.py`)

### `_verify_request_recaptcha_token(self, action)` [@ANCHOR: form_shield:captcha_hook]

Replaces Google reCAPTCHA for the actions `website_form`, `website_event_registration`, `website_mass_mailing_subscribe` and `website_mail_follow` with the shield; any other action (notably `login`) gets stock behaviour. It pops `recaptcha_token_response` from the request, runs `form.shield.check(action, response, own_cursor=True)` and returns `None` when it is ok; otherwise it raises `UserError` with the outcome's message ("We could not verify this submission...", "Too many submissions from your network...", and so on). The count is written on its own cursor because raising rolls the request back. Test: `test_odoo_forms_require_a_solved_challenge`.

### Templates (`views/form_shield_templates.xml`)

### `form_shield.fields` template [@ANCHOR: form_shield:fields_template]

Dropped inside a `<form data-form-shield="FORM_ID">` with `t-call` (optional `fallback_url` pointing at the page that serves the question form; `issued` for the question variant). It holds a hidden `shield_response` input, the honeypot `fs_homepage` moved off-screen rather than `display:none` (some programs skip hidden fields), a polite live status line `data-form-shield-status` that screen readers announce, a hidden link to `fallback_url`, a hidden question box (`shield_challenge`, `shield_answer`) and, with `fallback_url`, a `noscript` paragraph saying the check needs scripts. Test: `test_report_form_happy_path_tour`.

### `form_shield.refused_page` template [@ANCHOR: form_shield:refused_page]

Takes `verdict` and `back_path`; renders a website page titled "Too many submissions" for a `limited` verdict or "Temporarily unavailable" otherwise, with a sentence of explanation and a "Go back" link to `back_path`. `refusal_response` sets the 429 or 503 status and `no-store`. No form data is shown. Test: `test_refusal_response_by_outcome`.

### `form_shield.question_fields` template [@ANCHOR: form_shield:question_fields_template]

The question form for visitors without scripts or Web Worker support; `issued` is `form.shield.issue("q")`. It renders a hidden `shield_challenge`, the off-screen honeypot, and a labelled numeric text input `shield_answer` (required, `aria-describedby` a help text) whose label is the question, as plain text for a screen reader. Test: `test_question_form_page_renders_accessible_question`.

### Proof-of-work solver in the browser (`static/src/pow/pow_core.js`)

`pow_core.js` is a plain script, not an Odoo module, that sets `globalThis.FormShieldPow = {sha256Words, hex, leadingZeroBits, createSolver, solve}`; one file serves the Web Worker (`importScripts`), the page bundle (main-thread fallback) and the hoot test. It writes out SHA-256 because `crypto.subtle` is asynchronous and far too slow to call a million times. It computes exactly what `tokens.pow_digest` does.

### `pow_core.js` as a whole [@ANCHOR: form_shield:pow_core_js]

The whole file: the SHA-256 constants and the helpers below, and the export object. It touches no DOM, network or storage, holds module-level scratch arrays (`W`, so it is not re-entrant across threads of one realm, which is fine: one worker solves one challenge at a time), and runs identically in a worker and in the page. Test: `test_pow_core_js_solves_the_shared_vector`, which checks the vector that `tests/test_tokens.py` also asserts.

### `rotr(value, bits)` [@ANCHOR: form_shield:pow_rotr] Arguments: `value` a number treated as an unsigned 32-bit integer, `bits` the rotation, 1 to 31 (0 or 32 would shift by 32 and is not used). The result may be negative as a JavaScript number (the bitwise operators return signed 32-bit values); callers only combine it with further bitwise operations.

`-> number`. 32-bit rotate right: `(value >>> bits) | (value << (32 - bits))`. `bits` 1 to 31. Pure; used by `compress`. Test: `test_pow_core_js_solves_the_shared_vector`.

### `compress(state, bytes, offset)` [@ANCHOR: form_shield:pow_compress]

Folds the 64-byte block at `bytes[offset..offset+64]` into `state`, a `Uint32Array(8)` that is updated in place: the SHA-256 message schedule (64 words) and 64 rounds. Returns nothing; the shared array `W` is its scratch. Passing less than 64 bytes reads `undefined` and corrupts the state; callers always pass whole blocks. Test: `test_pow_core_js_solves_the_shared_vector`.

### `asciiBytes(text)` [@ANCHOR: form_shield:pow_ascii_bytes]

`-> Uint8Array`. One byte per character, `charCodeAt(i) & 0x7f`. Challenges and form ids are ASCII, so this equals the UTF-8 encoding the server hashes; a non-ASCII character would be folded, which cannot happen for a signed challenge. Pure. Test: `test_pow_core_js_solves_the_shared_vector`.

### `sha256Words(text)` [@ANCHOR: form_shield:pow_sha256_words]

`-> Uint32Array(8)`. The SHA-256 of an ASCII string as 8 big-endian words: it pads the message (0x80, zeros, 64-bit length) and runs `compress` per block. For tests and small uses; the solver uses a precomputed midstate instead. Pure. Test: `test_pow_core_js_solves_the_shared_vector`.

### `hex(words)` [@ANCHOR: form_shield:pow_hex] Input is any iterable of numbers such as the `Uint32Array(8)` returned by `sha256Words`; each is converted with `>>> 0` first, so a negative word prints as its unsigned value. It does not truncate or validate the length. Returns the 64-character lowercase digest for a SHA-256 result.

`-> string`. The words as a lowercase hex string, eight characters per word (`(word >>> 0).toString(16).padStart(8, "0")`). Used by tests to compare with Python's `hexdigest`. Pure. Test: `test_pow_core_js_solves_the_shared_vector`.

### `leadingZeroBits(words)` [@ANCHOR: form_shield:pow_leading_zero_bits] It reads only the first 8 words, because a SHA-256 result has exactly 8; it is the hot comparison inside `step`, called once per trial, so it avoids allocation. Argument `words` is the shared `trial` `Uint32Array(8)` or any array of eight numbers. It matches Python's `leading_zero_bits` on the same digest.

`-> number`. How many zero bits the first 8 words start with: 32 for each all-zero word, then `Math.clz32` of the first non-zero one (256 for an all-zero digest). Pure. Test: `test_pow_core_js_solves_the_shared_vector`.

### `createSolver(challenge, formId, difficulty)` [@ANCHOR: form_shield:pow_create_solver]

`-> {step(count), tried}`. A resumable solver. The fixed prefix `challenge:formId:` is hashed once (whole blocks into a midstate); each trial rewrites only the last one or two blocks with the decimal counter. `step(count)` tries up to `count` more counters from where it stopped and returns the winning (smallest) counter or `null`; `tried` is the next counter to try. Synchronous and CPU-bound, so callers slice it (the main-thread fallback uses 20000 per slice and yields with `setTimeout`). Test: `test_pow_core_js_solves_the_shared_vector`.

### `solve(challenge, formId, difficulty)` [@ANCHOR: form_shield:pow_solve]

`-> number`. A blocking solve: calls `step(50000)` repeatedly until a counter wins. For the worker and the tests only, since it blocks its thread; it never returns for a difficulty that is unreachable, which the server prevents by capping difficulty at 28. Test: `test_pow_core_js_solves_the_shared_vector`.

### Web Worker entry (`static/src/pow/pow_worker.js`) [@ANCHOR: form_shield:pow_worker_js]

Served as this plain static file (not a `blob:` URL, not inside an asset bundle), so a `worker-src 'self'` and `script-src 'self'` content-security policy admits it. It `importScripts` `pow_core.js`, and its `onmessage` handler takes `{challenge, formId, difficulty}`, calls `FormShieldPow.solve` and posts `{counter}`, or `{error: String(error)}` if solving throws. One message in, one message out; the page terminates the worker after the answer. Test: `test_pow_worker_runs_in_a_real_browser`.

### Browser session (`static/src/js/shield_client.js`)

The browser half of the shield: fetch a challenge, solve it in a Web Worker (or on the main thread when there is none or the worker cannot start), and give the form a `<challenge>~<counter>` answer once the server's minimum age has passed. Self-hosted: it loads nothing from and contacts nothing but this site.

### `ShieldSession` class [@ANCHOR: form_shield:client_session]

The object a form or the reCAPTCHA patch holds for one submission. State: `formId` (or `null`), `challenge` (a promise of the issued dict), `solution` (a promise of the counter when the form is known up front), `fetchedAt` (ms from `performance.now()`), and the injectable `fetchImpl`, `waitImpl`, `solveImpl` (tests only). Methods: `start`, `_solveFor`, `token`. It is created per submission and discarded after use; a challenge may be used once. Test: `test_client_session_solves_and_waits`.

### `fetchChallenge(kind = "pow", fetchImpl = fetch)` [@ANCHOR: form_shield:fetch_challenge]

`async -> object`. POSTs `kind` as a form body to `/form_shield/challenge` with `same-origin` credentials and `cache: "no-store"` and returns the parsed JSON. A non-2xx answer throws `ShieldUnavailable("/form_shield/challenge answered <status>")`; a network error rejects with the browser's error. Test: `test_client_session_solves_and_waits`.

### `solveOnMainThread(challenge, formId, difficulty)` [@ANCHOR: form_shield:solve_on_main_thread]

`-> Promise<number>`. Solves on the page's thread in slices of 20000 counters, yielding with `setTimeout(0)` between slices so the page stays responsive. Rejects with `ShieldUnavailable("pow_core.js is not loaded")` when `globalThis.FormShieldPow` is missing. Used when the browser has no Web Worker or the worker fails. Test: `test_client_session_solves_and_waits`.

### `solveInWorker(challenge, formId, difficulty)` [@ANCHOR: form_shield:solve_in_worker]

`-> Promise<number>`. Starts `new Worker("/form_shield/static/src/pow/pow_worker.js")`, posts `{challenge, formId, difficulty}` and resolves with the counter in the reply. The worker is terminated after the reply or on error. Rejects with `ShieldUnavailable` for a worker `error` reply or `onerror`, and with the constructor's error when the worker cannot start (a content-security policy, for example); `solve` then falls back to the main thread. Test: `test_pow_worker_runs_in_a_real_browser`.

### `solve(challenge, formId, difficulty)` [@ANCHOR: form_shield:shield_solve]

`-> Promise<number>`. Chooses how to solve: resolves `0` at once when there is no challenge or the difficulty is 0 (a question challenge needs no work); uses the main thread when `Worker` is undefined; otherwise tries the worker and falls back to the main thread if the worker fails. Test: `test_difficulty_zero_needs_no_work`.

### `defaultWait(milliseconds)` [@ANCHOR: form_shield:default_wait] Argument `milliseconds` is a non-negative number (the session passes `max(0, min_age * 1000 + 400 - age)`); zero resolves on the next timer turn. It never rejects. It is exported as a constant so a test can pass its own `waitImpl` instead, and it is the only timer the session uses.

`-> Promise<void>`. Resolves after `milliseconds` with `setTimeout`; the default delay the session waits for the server's minimum age. Replaced in tests by an instant stand-in. Test: `test_default_wait_waits_then_resolves`.

### `new ShieldSession(formId = null, {fetchImpl, waitImpl, solveImpl} = {})` [@ANCHOR: form_shield:session_constructor]

Stores `formId`, the injectable stand-ins (production passes none) and calls `start()`. `formId` is the form the answer is for when known now, so solving can start at once; `null` to learn it at `token()` time (Odoo's own forms know the action only when they ask). Returns the session; it begins a network fetch immediately. Test: `test_client_session_solves_and_waits`.

### `start()` [@ANCHOR: form_shield:session_start]

Fetches a challenge (`fetchChallenge("pow")`), records `fetchedAt` when it arrives, and, when the form id is known, starts solving it. It marks both promises as handled so a failure nobody has awaited yet is not reported as an unhandled rejection; `token()` awaits them and throws. Called by the constructor; calling it again discards the old challenge. Test: `test_client_session_solves_and_waits`.

### `_solveFor(formId)` [@ANCHOR: form_shield:session_solve_for] It is an `async` method of `ShieldSession` and uses `this.solveImpl`, the real `solve` unless a test supplied another. The solving itself runs in a Web Worker or on the main thread in slices, so awaiting it does not freeze the page. It performs no network access of its own beyond awaiting the earlier fetch.

`async -> number`. Waits for the challenge and solves it for `formId` with `solveImpl` (the real `solve` by default). Rejects when the fetch or the solve fails. Test: `test_client_session_solves_and_waits`.

### `token(formId = this.formId)` [@ANCHOR: form_shield:session_token]

`async -> string`. The `<challenge>~<counter>` answer for `formId`. It awaits the challenge, uses the solution already started when `formId` matches (otherwise solves now), then waits until the challenge is at least `min_age` seconds old plus a 400 ms margin so a fast machine is never too early, and returns the answer. Throws `ShieldUnavailable` or the underlying error on failure. One answer, one use: the caller makes a new session for the next submit. Test: `test_client_session_solves_and_waits`.

### Form wiring (`static/src/js/form_shield_form.js`)

### `FormShieldForm` interaction [@ANCHOR: form_shield:form_interaction]

An Odoo public interaction on `form[data-form-shield]` registered as `form_shield.FormShieldForm`. The form carries a hidden `shield_response` input, a polite status element and an optional hidden fallback link (all provided by the `form_shield.fields` template). Solving starts when the form is shown (when it sits in a Bootstrap modal, when the modal opens) and never blocks typing. Pressing send before it finishes holds the submit, says so in the status line, and sends the form (`requestSubmit`, keeping the submitter) the moment the answer is ready. Test: `test_report_form_happy_path_tour`.

### `setup()` [@ANCHOR: form_shield:form_setup] The framework calls it before `start()`; it reads `this.el.dataset.formShield`, so a form without the attribute would not have matched the `form[data-form-shield]` selector at all. It makes no network request and changes no DOM.

Reads the form id from `data-form-shield`, and initialises `session = null` and `released = false` (`released` means the submit may go through unheld). Called once by the interaction framework. Test: `test_report_form_happy_path_tour`.

### `start()` [@ANCHOR: form_shield:form_start] It returns nothing. In a modal, `begin()` can run again each time the modal is shown (it clears and restarts the check, so a visitor who closes and reopens gets a fresh challenge); listeners are registered with `addListener` so the framework removes them when the interaction is destroyed.

If the form is inside a `.modal`, waits for its `show.bs.modal` event and then calls `begin()` (so a challenge is fetched only when the visitor opens the modal); otherwise calls `begin()` at once. Test: `test_report_form_happy_path_tour`.

### `statusEl` [@ANCHOR: form_shield:form_status_el] The status element is a polite live region (`role="status"`, `aria-live="polite"`) supplied by the `form_shield.fields` template; the getter queries the DOM each time rather than caching, so a form re-rendered by another script is still found. Read-only; no arguments.

A getter returning the form's `[data-form-shield-status]` element, or `null` when the form has none. Test: `test_report_form_happy_path_tour`.

### `say(text)` [@ANCHOR: form_shield:form_say] `text` is already translated (callers pass `_t(...)` results). It replaces the element's previous text, so only the latest message is announced. It returns nothing and cannot fail; with no status element the message is silently dropped and the form still works.

Sets the status element's text to `text` (a translated string) so a screen reader announces it through its polite live region; does nothing when there is no status element. Test: `test_report_form_happy_path_tour`.

### `begin()` [@ANCHOR: form_shield:form_begin]

Starts the check. For a signed-in member (`session.is_website_user` false) it only sets `released = true`: the server checks members by honeypots and rate limits, so their form is never held. For a visitor it clears the answer field, says "Verifying you are human…", makes a `ShieldSession(formId)` and stores `pending`, the promise of the answer: on success it fills `shield_response` and says "Verified. You can send the form."; on failure it calls `unavailable()` and resolves `null`. Test: `test_report_form_happy_path_tour`.

### `onSubmit(ev)` [@ANCHOR: form_shield:form_on_submit]

The submit handler. When the form is released or there is no pending check it does nothing and the form posts. Otherwise it cancels the submit, says the form will be sent as soon as the check finishes, sets `aria-busy`, waits for the answer, clears `aria-busy`, and, if an answer arrived, sets `released = true` and calls `requestSubmit(submitter)` so the form posts with the answer. If there is no answer it stays unsent and the status line explains why. Test: `test_report_form_happy_path_tour`.

### `notice_text` (`static/src/js/error_notice.js`) [@ANCHOR: form_shield:notice_text]

`noticeText(reason) -> string`. The words shown for a `form_shield_error` reason: expired, `ip_changed`, `too_fast`, `wrong_answer`, `no_token`, `rate_subject`, `rate_target`, `rate_global` and `unavailable` each have a sentence; any other reason gets "We could not send the form. Please try again." The strings are lazy translations. Pure. Test: `test_error_notice_messages`.

### `FormShieldErrorNotice` interaction [@ANCHOR: form_shield:error_notice]

Registered as `form_shield.FormShieldErrorNotice` on `#wrapwrap`. A shielded form that refuses a person redirects back with `?form_shield_error=<reason>`; this shows the reason's words in a danger notification titled "Not sent" and removes the parameter from the address with `history.replaceState`. Reasons that identify a program are never redirected with: those get the form's ordinary success answer. Test: `test_error_notice_messages`.

### Odoo's own forms, browser side (`static/src/js/recaptcha_patch.js`) [@ANCHOR: form_shield:recaptcha_patch]

Patches `ReCaptcha.prototype` of `google_recaptcha` so Odoo's website form, event registration, newsletter and follow forms get the shield's answer as their token and load no Google script. The enforced actions (`website_form`, `website_event_registration`, `website_mass_mailing_subscribe`, `website_mail_follow`) are listed in both this file and `models/ir_http.py` and must be kept equal; any other action gets an answer with no token. Details are in the two entries at the end of this file (`loadLibs` and `getToken`). Test: `test_odoo_forms_require_a_solved_challenge`.

### `report_limits` in `user_websites` [@ANCHOR: user_websites:report_limits]

`REPORT_LIMITS` and `REPORT_MEMBER_LIMITS` in `user_websites/controllers/main.py`, passed to `form.shield.check_form` as `limits`: a guest is allowed 5 reports per hour per network, a member 30 per hour per user; both 10 per day per reported page (a page that is really a problem is reported by several people, not hundreds) and 100 per hour for the whole site. The older limit counted `request.remote_addr`, which behind the Cloudflare tunnel is always the loopback address, so every visitor shared one counter. Also `MIN_DESCRIPTION_CHARS = 10` and `MAX_URLS_FROM_GUEST = 2`. Test: `test_report_violation_is_rate_limited`.

### `normalize_report_target(raw, hosts)` [@ANCHOR: user_websites:normalize_report_target]

`-> str | None`. The reported URL in one canonical form, or `None` when it is not a page of this site. `raw` is the posted URL; `hosts` is the set from `_own_site_hosts()`. It refuses empty text, more than 2048 characters, control characters, URLs with credentials, schemes other than http and https, and any host not in `hosts` (a leading `www.` is ignored); http and https are the same page (the tunnel makes the request see http); a bare path (starting with a single `/`) means this host; the fragment is dropped. The result is `origin + path + ?query`. Reads the request host for bare paths. Tests: `test_normalize_report_target`, `test_report_violation_refuses_another_sites_url`.

### `report_problem(target, description, email, member)` [@ANCHOR: user_websites:report_problem]

`-> str | None`. The reason a report is refused on its content alone, else `None`; deliberately only structural, a rude or wrong report is for the moderators to read. In order: no `target` gives `foreign_target`; a description under 10 characters gives `short_description`; a single token of 16 or more random-looking characters containing lower case, upper case and a digit gives `gibberish`; then, for a guest only, more than two links gives `too_many_urls` and an address that fails strict normalisation or has a bad local part (leading or trailing dot, `..`) gives `bad_email`. A signed-in member is not asked for an address or checked for links. Pure. Test: `test_report_problem_reasons`.

### Report page route [@ANCHOR: user_websites:report_violation_page]

`UserWebsitesController.report_violation_form(self, url="", **_ignored)`, route `GET /website/report_violation`, `auth="public"`, `website=True`, no sitemap entry. The report form as a page of its own with the plain-text question in place of the proof of work: it serves a visitor whose browser runs no scripts (the Bootstrap modal cannot open) or cannot run the proof of work (the modal links here), and is what a post that carried no answer at all is shown. `url` defaults to the `Referer` header. A question is issued per request, so the page is `no-store` and opts into no edge caching. Test: `test_question_form_page_renders_accessible_question`.

### Report shield step [@ANCHOR: user_websites:report_violation_shield]

Inside `UserWebsitesController.report_violation` (`POST /website/report_violation`, `csrf=True`): after the `website_honeypot` check and the missing-field check it normalises the target with `normalize_report_target(url, _own_site_hosts())`, then calls `form.shield.check_form("report_violation", post, target=target, limits=REPORT_MEMBER_LIMITS or REPORT_LIMITS)`. A refused verdict goes to `_report_refusal` (a program is told it succeeded; a person is told what to do and keeps what they typed). Only after the shield passes do the structural `report_problem` checks run, so a program that cannot solve the proof of work never reaches them. Test: `test_report_violation_shield_outcomes`.

## Page script: the refusal notice

- `FormShieldErrorNotice.start()` runs on each page; when the address carries `form_shield_error=<reason>` it shows the reason's words in a danger notification and removes the parameter from the address, keeping the others, so a reload does not repeat it; without the parameter it does nothing. Tests: `static/tests/error_notice_start.test.js`. [@ANCHOR: form_shield:js_error_notice_start]

## Page script: a form whose check cannot run

- `FormShieldForm.unavailable()` is what a shielded form does when the browser cannot run the automatic check: it shows the link to the simple question page when the form has one; otherwise it asks the question inside the form and lets the form be sent with the answer; with neither, or when the question cannot be fetched, it says so in the form's status line. Tests: `static/tests/form_unavailable.test.js`. [@ANCHOR: form_shield:js_form_unavailable]

## Page script: Odoo's own forms

- `ReCaptcha.loadLibs()` (patched) loads no Google script; for a visitor who is not signed in it starts a shield session so the challenge is fetched while the form is filled in. [@ANCHOR: form_shield:js_recaptcha_load_libs]
- `ReCaptcha.getToken()` (patched) solves a challenge for the actions the server enforces and returns it as the token, using a new session for the next submit; for any other action, or a signed-in member, it returns a message and no token; when the solve fails it returns an error. Tests: `static/tests/recaptcha_patch.test.js`. [@ANCHOR: form_shield:js_recaptcha_get_token]
