# SES Webhook Receiver

## Overview
A module to securely receive Amazon SNS webhooks containing SES incoming emails. It routes emails to the appropriate tenant company within Odoo by passing them to `mail.thread.message_process`.

## Developer Guide

### Key Components
- **`SesWebhookController` (`controllers/main.py`)**: Defines a public HTTP endpoint at `POST /mail/webhook/sns`. It receives incoming Amazon SNS webhooks.
  - **Authentication**: It authenticates requests via a URL query parameter `token`.
  - **Subscription Confirmation**: Automatically confirms SNS subscription requests by visiting the `SubscribeURL`.
  - **Notification Processing**: Parses SES notifications. If a valid `content` field containing a raw email is found, it encodes the email in UTF-8 and feeds it into the standard Odoo mail thread message_process method for the matched company.
- **`SesWebhookDomain` (`models/ses_webhook_domain.py`)**: Stores the domain name and maps it to a specific tenant company (`res.company`).
  - Auto-generates a secure `secret_token` upon creation.
  - Computes the webhook URL.
- **`SesWebhookLog` (`models/ses_webhook_log.py`)**: Logs webhook activity (Payload Type, Status, Raw Payload, Error Messages).
  - **Scheduled Action**: `_cron_truncate_logs` runs daily to remove logs older than 30 days to save database space.

### Usage
Developers extending this module do not typically need to call its functions directly. Instead, this module acts as an ingestion pipeline into the standard `mail.thread` mechanism. Developers should rely on the standard Odoo `message_new` and `message_update` methods in their models to process the ingested emails.
