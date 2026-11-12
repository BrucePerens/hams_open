# Data structures

### `FormShieldController` [@ANCHOR: form_shield:form_shield_form_shield_api_form_shield_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `challenge`. Defined in `form_shield/controllers/form_shield_api.py:8` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `challenge`.

**See also:** test `test_challenge_endpoint_issues_signed_uncacheable_json (tests/test_http.py)`.

### `FormShield` [@ANCHOR: form_shield:form_shield_form_shield_form_shield]

**Purpose and location.** Model description: Form shield (anti-spam checks for public forms). Defined in `form_shield/models/form_shield.py:58` as an Odoo model; bases: `models.AbstractModel`. (model `form.shield`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "form.shield"`; `_description = "Form shield (anti-spam checks for public forms)"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_int_param`, `_signing_key`, `_client_address`, `issue`, `_redis`, `_key`, `_reject`, `refuse`, `check`, `check_form`, `_check_response`, `_claim_nonce`, `_over_limit`.

**See also:** test `test_challenge_endpoint_issues_signed_uncacheable_json (tests/test_http.py)`.

### `FormShieldRejection` [@ANCHOR: form_shield:form_shield_form_shield_rejection_form_shield_rejection]

**Purpose and location.** One counter per day, form and reason. No address, no email, no text a visitor typed. Defined in `form_shield/models/form_shield_rejection.py:10` as an Odoo model; bases: `models.Model`. (model `form.shield.rejection`; default order "day desc, form_id, reason".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | no label or help in the source (compute=`"_compute_name"`) |
| `day` | `Date` | no label or help in the source (required=`True`; index=`True`) |
| `form_id` | `Char` | Form (required=`True`; index=`True`) |
| `reason` | `Char` | no label or help in the source (required=`True`; index=`True`) |
| `count` | `Integer` | no label or help in the source (required=`True`; default=`0`) |

**Class attributes.** `_name = "form.shield.rejection"`; `_description = "Form shield daily counters"`; `_order = "day desc, form_id, reason"`; `_day_form_reason_unique = models.Constraint(
        "unique(day, form_id, reason)", "One counter per day, form and reason."
 `.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `_count`, `_upsert`, `_cron_purge`.

**See also:** test `test_challenge_endpoint_issues_signed_uncacheable_json (tests/test_http.py)`.

### `IrHttp` [@ANCHOR: form_shield:form_shield_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_verify_request_recaptcha_token`. Defined in `form_shield/models/ir_http.py:36` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_verify_request_recaptcha_token`.

**See also:** test `test_honeypot_marks_a_program (tests/test_form_shield.py)`.

### `Parsed` [@ANCHOR: form_shield:form_shield_tokens_parsed]

**Purpose and location.** The fields of a challenge whose signature has been checked. Defined in `form_shield/tokens.py:108` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`.

**See also:** test `test_tampered_challenge_is_refused (tests/test_tokens.py)`.

### `FormShieldErrorNotice` [@ANCHOR: form_shield:form_shield_js_error_notice_form_shield_error_notice]

**Purpose and location.** The public-page interaction that shows the "Not sent" message when a form submission was refused by the form shield. Defined in `form_shield/static/src/js/error_notice.js` and registered in `public.interactions`; it attaches to `#wrapwrap`.

**Behaviour.** `start` looks for the query parameter `form_shield_error`. When it is present it shows a non-sticky danger notification titled "Not sent" whose text `noticeText` chooses from a fixed table by the parameter's value (an unknown value gets the default message), and then removes the parameter from the address with `history.replaceState`, so a reload does not show the notice again. Without the parameter it does nothing. The class has no fields: the notification service is the only state it uses, and the value from the URL is shown as text, never as markup.

**See also:** the tests in `form_shield/static/tests/error_notice_start.test.js`.

### `ShieldUnavailable` [@ANCHOR: form_shield:form_shield_js_shield_client_shield_unavailable]

**Purpose and location.** The error thrown when the shield challenge cannot be obtained or solved, so a form can tell "the shield is unavailable" from other failures. Defined in `form_shield/static/src/js/shield_client.js` as `export class ShieldUnavailable extends Error {}`, with no fields and no methods of its own.

**Behaviour.** `fetchChallenge` throws it when the challenge request answers with an error status, the solving code throws it when `pow_core.js` is not loaded or the worker fails; callers catch it and decide what to show. It carries only the standard `message` and `stack` of an `Error`.

**See also:** the tests in `form_shield/static/tests/form_shield.test.js` that make the fetch fail.

### `ShieldSession` [@ANCHOR: form_shield:form_shield_js_shield_client_shield_session]

**Purpose and location.** One form's proof-of-work session with the form shield: it fetches a challenge from `/form_shield/challenge`, solves it in a Web Worker and, after the server's minimum time-to-submit has passed, hands out the answer token for the form submission. Defined in `form_shield/static/src/js/shield_client.js`.

**Constructor.** `new ShieldSession(formId = null, { fetchImpl, waitImpl, solveImpl } = {})`. `formId` is the form the answer is for (when known the solving starts at once; `null` learns it at `token()` time). The three options replace `fetch`, the delay and the worker solve; they are for tests of this class only and production passes nothing.

| Field | Type | Meaning |
|---|---|---|
| `formId` | string or null | The form the answer is for. |
| `fetchImpl`, `wait`, `solveImpl` | functions | Replaceable fetch, delay and solver. |
| `challenge` | Promise | The issued challenge; rejected with `ShieldUnavailable` on failure (the rejection is also swallowed so an unused session raises no unhandled error). |
| `fetchedAt` | number or null | `performance.now()` when the challenge arrived; the age margin (400 ms past the server minimum) is counted from it. |
| `solution` | Promise or null | The solving in progress, started by `_solveFor(formId)`. |

**Methods.** `start()` begins a fresh challenge (also used to start over); `token(formId)` waits for the solution (solving again if `formId` differs from the one given at construction) and for the challenge's own minimum age (`min_age` seconds from the issued challenge plus a 400 ms margin, counted from `fetchedAt`), then returns the answer `<challenge>~<counter>`; it throws `ShieldUnavailable` on failure. What the server accepts (one use, binding to a form) is decided server side and is not claimed here.

**See also:** the tests `test_client_session_solves_and_waits` and the others in `form_shield/static/tests/form_shield.test.js`.
