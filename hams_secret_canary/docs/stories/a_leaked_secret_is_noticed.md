<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Leaked Secret Is Noticed

As the **Operator** of an Odoo server whose web process can read every secret it uses, I want a decoy credential or a
registered real secret that appears in a response or a log line to raise an alarm, so that the next bug that leaks a
secret is noticed when it happens and not months later.

1. When the module is installed, a per-install salt is generated and a decoy Stripe key, S3 secret and Cloudflare token
   are generated and stored as the system parameters `stripe_secret_key_old`, `s3_secret_access_key_old` and
   `cloudflare_api_token_old`. Their salted hashes are the registered canaries.
   `[@ANCHOR: hams_secret_canary:post_init_hook]` `[@ANCHOR: hams_secret_canary:plant_canaries]`
   `[@ANCHOR: hams_secret_canary:new_salt]` `[@ANCHOR: hams_secret_canary:make_canary]`
2. Code that holds a real secret registers it; only the salted hash is stored.
   `[@ANCHOR: hams_secret_canary:register_secret]` `[@ANCHOR: hams_secret_canary:fingerprint]`
   `[@ANCHOR: hams_secret_canary:is_token]` `[@ANCHOR: hams_secret_canary:load_state]` `[@ANCHOR: hams_secret_canary:store_state]`
3. Every finished text response is scanned for a registered token, and every log record is scanned as it is made.
   `[@ANCHOR: hams_secret_canary:post_dispatch_scan]` `[@ANCHOR: hams_secret_canary:scannable_response]`
   `[@ANCHOR: hams_secret_canary:scan_text]` `[@ANCHOR: hams_secret_canary:find_matches]` `[@ANCHOR: hams_secret_canary:candidates]`
   `[@ANCHOR: hams_secret_canary:register_hook]` `[@ANCHOR: hams_secret_canary:watch_log_records]`
4. A hit runs every alert backend: a CRITICAL log line, a security-log row and an email to the administrator. A log
   line that held the value is rewritten with the value removed. No alarm carries the matched value.
   `[@ANCHOR: hams_secret_canary:redact]` `[@ANCHOR: hams_secret_canary:raise_alarm]`
   `[@ANCHOR: hams_secret_canary:register_backend]` `[@ANCHOR: hams_secret_canary:logging_backend]`
   `[@ANCHOR: hams_secret_canary:record_backend]` `[@ANCHOR: hams_secret_canary:mail_backend]`

This detects a leak after it has happened. It does not prevent one.
