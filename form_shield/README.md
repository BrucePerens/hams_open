# form_shield

Self-hosted anti-spam for public forms. No Google reCAPTCHA, no Cloudflare Turnstile, no image puzzle,
no third-party script, no cookie. Reusable: any module's controller can put a form behind it with
two calls and one template.

## What a visitor sees

Nothing, mostly. When a shielded form opens, the browser asks `/form_shield/challenge` for a signed
challenge and solves it in a Web Worker (SHA-256 proof of work, about half a second on a laptop,
a few seconds on an old phone) while the visitor types. The form shows "Verifying you are human…"
in a `role="status" aria-live="polite"` line. Pressing Send before it finishes holds the form and
sends it the moment the answer is ready. Typing is never blocked.

A browser that cannot run it (no scripts, no Web Worker and a main-thread solve that fails) gets a
link to a plain page with one question in words ("What is seven plus four? Type your answer as
digits."), per visitor, readable by a screen reader, no image.

## What the server checks (`form.shield.check`)

In this order: honeypot fields (any text in one marks a program); for a guest, the challenge's
signature, expiry, network (a keyed hash of the /24 or /48), minimum age since it was issued (3 s,
6 s for the question), the proof of work, and that it has not been used before (Redis `SET NX`);
then rate limits per network (per user for a member), per target (the page being reported) and per
form. A signed-in member skips the challenge but not the honeypot or the limits.

Outcomes: `silent` (looks like a program: answered like a success, nothing stored), `retry` (expired,
network changed, too fast, wrong answer: the visitor is told), `question` (no answer was sent: show
the question page), `limited` (HTTP 429), `unavailable` (Redis is down: HTTP 503, never skipped).

## Using it

```python
verdict = request.env["form.shield"].check_form("my_form", post, target=normalized_url)
if not verdict.ok:
    ...   # act on verdict.outcome
```

```xml
<form method="post" data-form-shield="my_form">
    <t t-call="form_shield.fields"><t t-set="fallback_url" t-value="'/my/question/page'"/></t>
```

The question page renders `form_shield.question_fields` with `form.shield.issue("q")`.

Odoo's own forms (`/website/form/...`, event registration, newsletter subscribe, follow) are covered
without touching them: `static/src/js/recaptcha_patch.js` replaces what stands behind the stock
`ReCaptcha` class and `models/ir_http.py` verifies its token for the listed actions. `login` is not
enforced (see below).

## Privacy

Nothing is stored that names a visitor. Redis keys are keyed hashes of a network prefix and expire
within the rate-limit window (at most a day). The only database table, `form.shield.rejection`, holds
one counter per (day, form, reason) and is purged after `form_shield.retention_days` (90). Back
office: Settings, Form shield counters.

## Settings (system parameters)

`form_shield.difficulty` (19 leading zero bits), `form_shield.min_age_seconds` (3),
`form_shield.question_min_age_seconds` (6), `form_shield.ttl_seconds` (7200),
`form_shield.retention_days` (90). `form_shield.signing_key` is generated at install; never print it.

## Not done on purpose

`login` is not enforced: a bug there locks every member out and a password guess is already limited
by Odoo. Say the word and add `"login"` to `ENFORCED_ACTIONS` in `models/ir_http.py`.
