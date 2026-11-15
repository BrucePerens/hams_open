# Data structures

### `SesWebhookController` [@ANCHOR: ses_webhook:ses_webhook_webhook_api_ses_webhook_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `receive_sns_webhook`, `_handle_ses_event_notification`. Defined in `ses_webhook/controllers/webhook_api.py:255` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `receive_sns_webhook`, `_handle_ses_event_notification`.

**See also:** test `test_34_forged_complaint_without_signature_does_not_blacklist (tests/test_ses_webhook.py)`.

### `SesWebhookDomain` [@ANCHOR: ses_webhook:ses_webhook_ses_webhook_domain_ses_webhook_domain]

**Purpose and location.** Model description: SES (Amazon Simple Email Service) Webhook Domain Configuration. Defined in `ses_webhook/models/ses_webhook_domain.py:5` as an Odoo model; bases: `models.Model`. (model `ses.webhook.domain`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Domain Name: E.g., example.com (required=`True`) |
| `secret_token` | `Char` | Secret Token (required=`True`; default=`lambda self: secrets.token_urlsafe(24)`; copy=`False`) |
| `topic_arn` | `Char` | Expected SNS Topic ARN (when set, only messages signed for this topic are accepted) |
| `company_id` | `Many2one` | Tenant (Company) (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `webhook_url` | `Char` | Webhook URL (compute=`'_compute_webhook_url'`) |
| `log_ids` | `One2many` | Logs (comodel `ses.webhook.log`) |

**Class attributes.** `_name = 'ses.webhook.domain'`; `_description = 'SES Webhook Domain Configuration'`; `_name_uniq = models.Constraint("UNIQUE(name)", "The domain name must be unique!")`; `_token_uniq = models.Constraint("UNIQUE(secret_token)", "The secret token must be unique!")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_webhook_url`, `create`, `write`, `_sync_service_account_companies`.

**See also:** test `test_02_webhook_empty_or_invalid_payload (tests/test_ses_webhook.py)`.

### `SesWebhookLog` [@ANCHOR: ses_webhook:ses_webhook_ses_webhook_log_ses_webhook_log]

**Purpose and location.** Model description: SES Webhook Log. Defined in `ses_webhook/models/ses_webhook_log.py:4` as an Odoo model; bases: `models.Model`. (model `ses.webhook.log`; default order 'create_date desc'.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Message ID (required=`True`; readonly=`True`) |
| `payload_type` | `Selection` | Payload Type (values `Notification`, `SubscriptionConfirmation`, `UnsubscribeConfirmation`, `Unknown`; readonly=`True`) |
| `status` | `Selection` | Status (values `success`, `failed`, `ignored`, `rejected_subscribe_url`, `rejected_signature`, `rejected_topic`, `rejected_sender`; readonly=`True`) |
| `domain_id` | `Many2one` | Webhook Domain (comodel `ses.webhook.domain`; readonly=`True`; ondelete=`'cascade'`) |
| `company_id` | `Many2one` | mirrors the related field `'domain_id.company_id'` (comodel `res.company`; related=`'domain_id.company_id'`; store=`True`; readonly=`True`) |
| `error_message` | `Text` | Error Message (readonly=`True`) |
| `raw_payload` | `Text` | Raw JSON Payload (readonly=`True`) |

**Class attributes.** `_name = 'ses.webhook.log'`; `_description = 'SES Webhook Log'`; `_order = 'create_date desc'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_01_migration_unlinks_the_purge_crons_by_xmlid (tests/test_purge_cron_migration.py)`.

### `SesWebhookPendingSubmission` [@ANCHOR: ses_webhook:ses_webhook_ses_webhook_pending_submission_ses_webhook_pending_submission]

**Purpose and location.** docs/proposals/SES_WEBHOOK_SENDER_REGISTRATION.md section 3: holds an unregistered/unmatched sender's original inbound email content while they're nudged toward registering, rather than either silently creating a misattributed record (the old behavior) or silently dropping the message. Rows are kept forever, like ses.webhook.log.  Linking a since-registered user back to their pending submission via `token` is section 3's own explicitly-flagged open sub-question, not resolved here -- `consumed` exists as the field that flow would set, but nothing sets it yet. Defined in `ses_webhook/models/ses_webhook_pending_submission.py:10` as an Odoo model; bases: `models.Model`. (model `ses.webhook.pending_submission`; default order 'create_date desc'.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (compute=`'_compute_name'`; store=`True`) |
| `sender_email` | `Char` | Sender Email (required=`True`; index=`True`; readonly=`True`) |
| `raw_content` | `Text` | Raw Email Content (readonly=`True`) |
| `domain_id` | `Many2one` | Webhook Domain (comodel `ses.webhook.domain`; readonly=`True`; ondelete=`'cascade'`) |
| `company_id` | `Many2one` | mirrors the related field `'domain_id.company_id'` (comodel `res.company`; related=`'domain_id.company_id'`; store=`True`; readonly=`True`) |
| `token` | `Char` | Token (required=`True`; default=`lambda self: secrets.token_urlsafe(24)`; readonly=`True`; copy=`False`) |
| `consumed` | `Boolean` | Consumed (default=`False`; readonly=`True`) |

**Class attributes.** `_name = 'ses.webhook.pending_submission'`; `_description = 'SES Webhook: Pending Registration-Gated Submission'`; `_order = 'create_date desc'`; `_token_uniq = models.Constraint("UNIQUE(token)", "The token must be unique!")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `create_and_notify`.

**See also:** test `test_04_no_truncate_methods_and_service_account_cannot_delete (tests/test_purge_cron_migration.py)`.

### `SesWebhookLog._already_handled(domain_id, message_id)` [@ANCHOR: ses_webhook:already_handled]

`-> bool`, `@api.model`. Called by the webhook route for a Notification before it processes it. It runs a no-change `UPDATE ses_webhook_domain SET id = id` for the domain, then says whether a log row with this MessageId and domain and status `success` or `ignored` exists (a `failed` one is reprocessed). The UPDATE makes two simultaneous deliveries of one MessageId take turns: the later one fails to serialize (its snapshot cannot see the earlier log row, which is written after processing), Odoo runs the request again, and the search then finds the log and the route acknowledges without processing. A unique index cannot do this: failed and rejected attempts of one MessageId are logged too. Test: `test_a_simultaneous_double_delivery_is_not_processed_twice`.

### `_failed_sender_check(ses_message)` [@ANCHOR: ses_webhook:failed_sender_check]

`-> str`, module function in `controllers/webhook_api.py`. Reads the SES `receipt` verdicts of a Notification and returns the reason to refuse the mail, or `''` to accept it. A DMARC `FAIL`, or `FAIL` for both SPF and DKIM, is refused: the route logs it with status `rejected_sender` and never calls `message_process` (which would post as the user the From header names). `PASS`, `GRAY` (a domain without a DMARC record), `PROCESSING_FAILED`, a single SPF or DKIM `FAIL` and missing or malformed verdicts are accepted. Test: `test_27c_failed_sender_verdicts_refuse_the_mail`.

### `_verdict(ses_message, name)` [@ANCHOR: ses_webhook:verdict]

`-> str`, module function in `controllers/webhook_api.py`. Reads one SES receipt verdict of a Notification: `name` is `dmarcVerdict`, `spfVerdict` or `dkimVerdict`, and the result is that verdict's `status` (`PASS`, `FAIL`, `GRAY` or `PROCESSING_FAILED`). It returns `''` when the `receipt`, the named verdict or its `status` is missing or is not the expected type, so a malformed Notification never raises. `_failed_sender_check` is the only caller. Test: `test_27b_verdict_reads_the_status_or_empty`.
