# Story: Ingesting Inbound Email via SES/SNS Webhooks

## Context
Amazon SES delivers inbound email to this platform as an HTTPS webhook call
from Amazon SNS, rather than over SMTP directly. `ses_webhook` receives that
call, authenticates it, and hands the raw email off to Odoo's standard
`mail.thread.message_process` pipeline for the matched tenant company.

## The Solution
1. **Receiving the notification**: `POST /mail/webhook/sns` authenticates the
   request via a per-domain `secret_token` query parameter, auto-confirms SNS
   subscription requests, and decodes the raw MIME email from the SNS
   `content` field before handing it to `message_process`.
2. **Failing safe toward AWS**: SNS retries a webhook delivery indefinitely
   until it receives a 2xx response, so a processing failure on Odoo's side
   (a malformed payload, a `message_process` exception, an unmatched domain)
   must still return HTTP 200 -- otherwise SNS would hammer the same failing
   delivery forever. The broad `except Exception` around processing records
   the real failure in `ses.webhook.log` rather than swallowing it, but
   always returns 200 either way ([@ANCHOR: COMM_ses_webhook_process_catch_all]).
3. **Administration**: Administrators manage per-domain webhook configuration
   (secret tokens, computed webhook URLs) and review ingestion activity
   (payload type, status, raw payload, error messages) through the module's
   backend list/form views ([@ANCHOR: COMM_ses_webhook_views_render]).

## Impact
Inbound support and community email reaches the right tenant's Helpdesk and
mail threads without exposing SMTP credentials or a directly-reachable mail
server, and a transient processing failure degrades to a logged error rather
than an AWS retry storm.
