---
anchor: pager_duty:amqp_check
code_hash: sha256:5aeee6c37c10476daa3ef96d77cd2caa53529ab12a9dd742b0130f6f5d7928aa
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: generalized_monitor.py `amqp_check` (`pager_duty/daemon/generalized_monitor.py`)

Read 2026-11-11, whole function. Tier 2: Opens a short AMQP connection to a configured broker with the given credentials; the password is never in a result text.

1. WHEN `user` or `password` is empty THE function SHALL return `(False, ...)` naming RMQ_USER and RMQ_PASS without opening any connection.
2. OTHERWISE THE function SHALL connect with one attempt and the module's timeouts, open a channel, close the channel and then the connection, and return `(True, "OK")`.
3. ON failure THE function SHALL return `(False, text)`, the text naming the class: authentication failed, access denied (vhost), protocol error, timed out, connection refused, or connection failed; a stack timeout is matched by name because it is not an `AMQPError`.
4. THE password SHALL never appear in the text; the user name and vhost do appear in the authentication and access-denied texts.
5. WHATEVER happens a connection still open at the end SHALL get a clean close, and a failure of that close is only logged. HOLDS.
