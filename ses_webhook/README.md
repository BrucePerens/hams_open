# SES Webhook Receiver

## Overview
A module to securely receive Amazon SNS webhooks containing SES incoming emails. It routes emails to the appropriate tenant company within Odoo by passing them to `mail.thread.message_process`.

## Developer Guide

### Key Components
- **`SesWebhookController` (`controllers/webhook_api.py`)**: Defines a public HTTP endpoint at `POST /mail/webhook/sns`. It receives incoming Amazon SNS webhooks.
  - **Authentication (two independent layers, both required)**: (1) a per-domain URL query parameter `token`, matched against `ses.webhook.domain.secret_token`; (2) real AWS SNS message-signature verification (`_verify_sns_signature`) -- the request body's own `Signature`/`SigningCertURL`/`SignatureVersion` fields are checked against AWS's documented signing scheme (the canonical string-to-sign built per the message's `Type`, verified against the certificate fetched from `SigningCertURL` after confirming that URL is a real `sns.<region>.amazonaws.com` signing-cert host). A request failing either check is rejected with 403; a signature failure on an otherwise-valid token is logged with its own `rejected_signature` status for forensic visibility (a valid token but a forged/unsigned message is the exact scenario a leaked token alone can no longer forge).
  - **Subscription Confirmation**: Automatically confirms SNS subscription requests by visiting the `SubscribeURL`.
  - **Notification Processing**: Parses SES notifications. If a valid `content` field containing a raw email is found, it encodes the email in UTF-8 and feeds it into the standard Odoo mail thread message_process method for the matched company.
  - **AWS SNS signature verification** (`[@ANCHOR: ses_webhook:COMM_verify_sns_signature]`): infrastructure/plumbing, not a user-facing feature -- verifies `Signature` against the string-to-sign AWS documents for the message's own `Type`, using the certificate `[@ANCHOR: ses_webhook:COMM_fetch_sns_signing_cert]` fetches (and caches) from `SigningCertURL`. Fails closed on any missing field, non-AWS cert host, unreachable/expired certificate, unsupported `SignatureVersion`, or signature mismatch.
- **`SesWebhookDomain` (`models/ses_webhook_domain.py`)**: Stores the domain name and maps it to a specific tenant company (`res.company`).
  - Auto-generates a secure `secret_token` upon creation.
  - Computes the webhook URL.
- **`SesWebhookLog` (`models/ses_webhook_log.py`)**: Logs webhook activity (Payload Type, Status, Raw Payload, Error Messages).
  - **Scheduled Action**: `_cron_truncate_logs` runs daily to remove logs older than 30 days to save database space.

### Usage
Developers extending this module do not typically need to call its functions directly. Instead, this module acts as an ingestion pipeline into the standard `mail.thread` mechanism. Developers should rely on the standard Odoo `message_new` and `message_update` methods in their models to process the ingested emails.
