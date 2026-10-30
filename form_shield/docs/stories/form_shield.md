# Story: Public Forms Are Protected From Programs Without a Third-Party Captcha

As the **Site Owner**, I want **the forms that anyone can send (report a violation, request an invitation, contact us, newsletter, event
registration) to turn away programs without a Google or Cloudflare captcha**, so that my moderators read people's reports and my visitors are
not tracked by a third party to prove they are people.

## Scenario: A visitor reports a page

The Report Violation button opens a modal on a page that may come from Cloudflare's edge cache. As the modal opens, the browser asks
`/form_shield/challenge` for a challenge of its own [@ANCHOR: form_shield:challenge_route] and solves it in a Web Worker
[@ANCHOR: form_shield:pow_worker_js] [@ANCHOR: form_shield:pow_core_js], telling the visitor "Verifying you are human…" in a polite live region
[@ANCHOR: form_shield:fields_template]. Typing is never blocked; pressing Send early waits for the answer and sends [@ANCHOR: form_shield:form_interaction]
[@ANCHOR: form_shield:client_session].

The functions behind those steps each carry their own anchor. The proof-of-work solver: [@ANCHOR: form_shield:pow_rotr] [@ANCHOR: form_shield:pow_compress] [@ANCHOR: form_shield:pow_ascii_bytes] [@ANCHOR: form_shield:pow_sha256_words] [@ANCHOR: form_shield:pow_hex] [@ANCHOR: form_shield:pow_leading_zero_bits] [@ANCHOR: form_shield:pow_create_solver] [@ANCHOR: form_shield:pow_solve]. The browser session: [@ANCHOR: form_shield:fetch_challenge] [@ANCHOR: form_shield:solve_on_main_thread] [@ANCHOR: form_shield:solve_in_worker] [@ANCHOR: form_shield:shield_solve] [@ANCHOR: form_shield:default_wait] [@ANCHOR: form_shield:session_constructor] [@ANCHOR: form_shield:session_start] [@ANCHOR: form_shield:session_solve_for] [@ANCHOR: form_shield:session_token]. The form wiring: [@ANCHOR: form_shield:form_setup] [@ANCHOR: form_shield:form_start] [@ANCHOR: form_shield:form_status_el] [@ANCHOR: form_shield:form_say] [@ANCHOR: form_shield:form_begin] [@ANCHOR: form_shield:form_on_submit]. The refusal wording shown after a redirect: [@ANCHOR: form_shield:notice_text].

## Scenario: The server decides

The challenge must be signed, unexpired, issued to the same /24 or /48 network, at least three seconds old, solved for this very form and
never used before; then the per-network, per-page and per-form limits are counted [@ANCHOR: form_shield:check] [@ANCHOR: form_shield:parse_challenge]
[@ANCHOR: form_shield:pow_valid] [@ANCHOR: form_shield:ip_tag]. A filled honeypot or a bad solution is answered like a success and stores nothing;
an expired challenge, a changed network or a wrong answer tells the visitor to try again; a rate limit answers 429; Redis being down answers 503.
A guest must give a valid address and say at least ten characters that are not a single random-looking string, link to the site's own pages, and
use at most two links [@ANCHOR: user_websites:report_problem] [@ANCHOR: user_websites:normalize_report_target] [@ANCHOR: user_websites:report_violation_shield].

## Scenario: A visitor cannot run the check

A browser with scripts off (the modal cannot open at all) or no Web Worker is offered a page with one question in words, new for each visit and
readable by a screen reader [@ANCHOR: user_websites:report_violation_page] [@ANCHOR: form_shield:question_valid] [@ANCHOR: form_shield:question_fields_template].

## Scenario: A signed-in member reports

A member skips the challenge and meets the honeypots and a looser limit, counted per user [@ANCHOR: user_websites:report_limits].

## Scenario: Odoo's own forms

The website form builder (Contact Us), event registration, newsletter and follow ask the stock `ReCaptcha` class for a token. `form_shield` makes
that token its own answer, loads no Google script, and checks it on the server where Odoo calls its captcha hook
[@ANCHOR: form_shield:recaptcha_patch] [@ANCHOR: form_shield:captcha_hook].

## Scenario: The administrator sees what is being turned away

Settings, Form shield counters shows how many submissions per day each form accepted and refused, by reason. It keeps no address, email or text, and
drops rows after 90 days [@ANCHOR: form_shield:count] [@ANCHOR: form_shield:purge] [@ANCHOR: form_shield:service_account_acl].

## Scenario: The pieces, for a developer

A form puts `form_shield.fields` inside a `<form data-form-shield="ID">` [@ANCHOR: form_shield:fields_template]. A controller calls
`check_form` [@ANCHOR: form_shield:check_form], which reads the standard fields and runs the checks [@ANCHOR: form_shield:check]; a refusal the
controller decides itself is counted the same way [@ANCHOR: form_shield:refuse]. A form that redirects hands the verdict to
`refusal_response` [@ANCHOR: form_shield:refusal_response], which sends a program to the success page, a person back with
`?form_shield_error=` (shown by [@ANCHOR: form_shield:error_notice]) and a rate limit or a Redis failure to a page with 429 or 503
[@ANCHOR: form_shield:refused_page]. Challenges are issued by [@ANCHOR: form_shield:issue]. The signing key is seeded once at install
[@ANCHOR: form_shield:post_init_hook], and the daily counters are purged by a cron [@ANCHOR: form_shield:cron_purge].

## Scenario: A visitor reports a listing, a repeater, an event or a forum post

Those forms (hams.com's `ham_classifieds`, `ham_repeater_dir`, `ham_events`, `ham_forum_extension`) and the request-an-invite page use the same shield
with their own form ids, their own per-target limits, and, on pages that can hold it, the question variant at `?q=1`.

## Scenario: Internals a maintainer can rely on

The signing key and tunables are read through one helper each [@ANCHOR: form_shield:signing_key] [@ANCHOR: form_shield:int_param]; the visitor's network
comes from the trusted-client-address utility, never from a header the visitor can forge [@ANCHOR: form_shield:client_address]. A challenge is built and
signed by [@ANCHOR: form_shield:make_challenge] with [@ANCHOR: form_shield:mac]; the network is stored only as a keyed hash [@ANCHOR: form_shield:ip_tag_hash];
the parsed challenge is a small value object [@ANCHOR: form_shield:parsed_challenge]; an answer is `challenge~solution` [@ANCHOR: form_shield:split_response].
The proof of work hashes `challenge:form:counter` [@ANCHOR: form_shield:pow_digest], counts leading zero bits [@ANCHOR: form_shield:leading_zero_bits], and the Python
twin of the browser's solver is [@ANCHOR: form_shield:solve_pow]. The question variant derives its two numbers from the signed nonce
[@ANCHOR: form_shield:question_numbers] [@ANCHOR: form_shield:question_text]. A guest's answer is checked in one place [@ANCHOR: form_shield:check_response]; a
challenge may be used once [@ANCHOR: form_shield:claim_nonce]; the limits count in Redis [@ANCHOR: form_shield:over_limit] under per-database keys
[@ANCHOR: form_shield:redis_key] on the shared connection [@ANCHOR: form_shield:redis_connection]; every refusal is counted by reason [@ANCHOR: form_shield:reject]
[@ANCHOR: form_shield:count_upsert] and the counters show a readable name [@ANCHOR: form_shield:rejection_name]. Redirecting forms build their paths with
[@ANCHOR: form_shield:with_query].
