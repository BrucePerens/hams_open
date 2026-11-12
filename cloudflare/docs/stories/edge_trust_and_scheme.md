# Story: Trusting the Cloudflare Edge Correctly

As a **Developer/Administrator**,
I want Odoo to know a request really is HTTPS, and to trust Cloudflare's own edge headers only
from a peer that is genuinely Cloudflare, so that session cookies, redirects, and IP-based trust
decisions are correct regardless of which real deployment topology (Tunnel, or plain reverse-proxy)
this installation uses.

Every numbered step has an entry below, under the heading that carries its anchor. The code is `wsgi_proxy_scheme.py` (a monkeypatch of the WSGI entry point), `models/trusted_ip_ranges.py` (the peer-trust model `cloudflare.trusted_ip_utils`), `models/res_config_settings.py` (the Settings fields and buttons) and `models/website.py` (credential cache busting).

## Scenario: HTTPS detection behind a Cloudflare Tunnel

1. `cloudflared` forwards to the origin over plain loopback HTTP and sends `CF-Visitor:
   {"scheme":"https"}` rather than `X-Forwarded-Proto`, so Odoo's own `request.httprequest.scheme`
   would otherwise never report "https" at all behind this deployment's real edge -- wrong for
   session-cookie `Secure` flags, absolute-URL generation, and redirects alike.

### The scheme fix as a whole [@ANCHOR: wsgi_proxy_scheme_fix]

`wsgi_proxy_scheme.py` runs once, at import time of the `cloudflare` module, and replaces `odoo.http.Application.__call__` (the WSGI entry point) with a wrapper. The wrapper sets `environ["wsgi.url_scheme"]` before Werkzeug builds `request.httprequest`, so every reader of `request.httprequest.scheme` (the session cookie `Secure` flag, absolute URLs, redirects) sees the truth. Odoo's own `ProxyFix` path cannot help here: it runs only when `X-Forwarded-Host` is present, and neither the local tunnel simulator nor the real edge sends it. It has no arguments and no return value of its own; its effect is the patched entry point described under `wsgi_proxy_scheme_fix_call`. A hook that runs before the database exists must not use the ORM; that is why peer trust is read from Redis (see `is_trusted_cf_peer`). There is no formal Odoo extension point for this, hence the monkeypatch. Test: `test_01_trusted_loopback_cf_visitor_https_sets_wsgi_url_scheme` in `tests/test_wsgi_proxy_scheme.py`.

2. The fix patches `odoo.http.Application.__call__`, the actual WSGI entry point, so it can set the
   raw `environ["wsgi.url_scheme"]` before Werkzeug's `Request.scheme` is ever read -- but only when
   the request's peer is trusted (loopback, the Tunnel case; or a Cloudflare-published/admin-custom
   IP range, the non-Tunnel "orange-cloud" case), so an untrusted peer can never forge a plain-HTTP
   connection into looking like HTTPS.

### The patched entry point [@ANCHOR: wsgi_proxy_scheme_fix_call]

`_patched_application_call(self, environ, start_response, *args, **kwargs)` is the wrapper. `environ` is the WSGI environ dict, mutated in place; `start_response` and the rest are passed through unchanged, and the result of the original `__call__` is returned. Steps: (1) `strip_client_forwarded_host` (below). (2) If the peer in `REMOTE_ADDR` passes `is_trusted_cf_peer`, take the scheme from the first entry of `X-Forwarded-Proto` if it is `http` or `https`, otherwise from the JSON `scheme` of `CF-Visitor` (a malformed header is logged at info level and ignored). A real edge sends both headers and the upstream value wins. (3) When a scheme was found, set `wsgi.url_scheme` and default `HTTP_X_FORWARDED_PROTO` to it. A peer that is not trusted changes nothing, so a visitor cannot forge it; at worst a forged header from a trusted-looking peer would make Odoo believe plain HTTP is HTTPS, and the browser would then refuse the `Secure` cookie. No locks, no I/O except the cached Redis read behind the trust check, no exceptions raised for bad input. Tests: `test_01b` to `test_09` in `tests/test_wsgi_proxy_scheme.py`.

3. That peer-trust check itself -- is this REMOTE_ADDR loopback, or inside the effective trusted-IP
   range list -- is shared, env-less logic the WSGI hook can call before any Odoo database cursor
   exists for the request.

### Peer trust without an environment [@ANCHOR: is_trusted_cf_peer]

`_is_trusted_cf_peer(remote_addr)` in `wsgi_proxy_scheme.py` takes the peer address string from `REMOTE_ADDR` and returns a boolean. It parses it with the shared peer parser (an IPv4-mapped IPv6 address is unmapped; an unparseable or missing value gives `False`). Loopback is trusted at once with no cache or Redis access, which covers the Tunnel case. Any other peer is trusted only if it falls inside a network of the cached trusted list, read through `_get_cached_trusted_networks()`: a per-worker list with a 60 second time-to-live, refilled from the Redis key `cloudflare:trusted_ip_ranges` under a lock. The cache starts empty (loopback only). If Redis is down or its content is malformed, the last good list is kept, and it is never replaced by Cloudflare's baked-in ranges. Safe to call on every request. Tests: `test_05b` to `test_05e` and `test_13` in `tests/test_wsgi_proxy_scheme.py`.

3a. A visitor's own `X-Forwarded-Host` header is dropped from every request that carries a Cloudflare
   marker header (`Cf-Ray`, `Cf-Connecting-Ip`, `Cf-Visitor`), before Odoo's `proxy_mode` handling can
   replace `Host` with it. Cloudflare passes the visitor's copy through and `cloudflared` only sets the
   header when an ingress rule configures `httpHostHeader` (none does), so on this deployment the header can
   only be forged. Without this a visitor of one hostname could make Odoo answer as another and Cloudflare
   would cache that answer under the requested name.

### Strip a forged `X-Forwarded-Host` [@ANCHOR: cloudflare:strip_client_forwarded_host]

The first action of `_patched_application_call`, and the constant `_CLOUDFLARE_REQUEST_MARKERS = ("HTTP_CF_RAY", "HTTP_CF_CONNECTING_IP", "HTTP_CF_VISITOR")` that drives it. When any marker is in the WSGI environ, the key `HTTP_X_FORWARDED_HOST` is removed, whether or not the peer is trusted. That stops Odoo's `proxy_mode` from running Werkzeug's `ProxyFix(x_host=1)` with the visitor's value, which would replace `Host` and make one hostname's visitor receive another site's pages (confirmed live on 2026-10-03, with `crawler.hams.com`) that Cloudflare would then cache under the wrong name. A request without any marker (a local nginx that sets the header from `Host`) is left alone. Mutates the environ, returns nothing. Tests: `test_10`, `test_11` and `test_12` in `tests/test_wsgi_proxy_scheme.py`.

## Scenario: Trusting Cloudflare's published IP ranges for "orange-cloud" (non-Tunnel) deployments

4. A self-hosted admin running Cloudflare in front of Odoo WITHOUT a Tunnel has a real network peer
   (not loopback) on every request, so the loopback-only check above can never match it. This
   module merges an auto-fetched Cloudflare-published range list with an admin-supplied custom list
   so that case can trust CF-* headers too. The whole feature is
   off by default behind one explicit "Trust Cloudflare's Published IP Ranges" setting: a
   Tunnel-only deployment's only peer is loopback, so until the admin turns it on the effective
   list is empty, auto-fetched snapshot and custom additions alike.

### The trusted IP range model [@ANCHOR: trusted_ip_ranges]

The module docstring of `models/trusted_ip_ranges.py` and the abstract model `cloudflare.trusted_ip_utils` that it introduces. Its data: four `ir.config_parameter` keys, `cloudflare.trust_non_tunnel_peers` (the opt-in, off by default), `cloudflare.trusted_ip_ranges_auto` (the fetched list, newline separated), `cloudflare.trusted_ip_ranges_custom` (the admin's list) and `cloudflare.trusted_ip_ranges_last_refreshed`; two baked-in snapshots `DEFAULT_CLOUDFLARE_IPV4_RANGES` and `DEFAULT_CLOUDFLARE_IPV6_RANGES` (fetched 2026-09-26); and the Redis key `cloudflare:trusted_ip_ranges` (a JSON list of CIDR strings, kept three days). It reads parameters as the trusted-IP service account. Turning the opt-in on only makes Odoo believe `CF-*` headers from a peer in those ranges; since the ranges front every Cloudflare customer, it means nothing unless the origin is also restricted to Cloudflare's edge (authenticated origin pulls or a zone firewall rule), which this module does not set up. A helper `_parse_ranges(text)` splits on whitespace and commas, skips and logs malformed tokens and stores networks in canonical form. Test: `test_00` to `test_03b` in `tests/test_trusted_ip_ranges.py`.

5. The auto-fetched half refreshes daily from `https://www.cloudflare.com/ips-v4`/`ips-v6`, seeded
   from a baked-in snapshot so the feature works before the cron's first run, and never overwrites
   the last known-good list on a failed/empty/malformed fetch.

### Daily refresh of the published ranges [@ANCHOR: cron_refresh_cloudflare_ip_ranges]

`_cron_refresh_cloudflare_ip_ranges()` on `cloudflare.trusted_ip_utils`, run by the scheduled action "Cloudflare: Refresh Trusted IP Ranges" once a day as the trusted-IP service user, and by the Settings "Refresh now" button. No arguments. It GETs the two fixed Cloudflare URLs (10 second timeout each, no caller-supplied URL), parses them, and only if both families returned at least one valid network does it store the combined list, the refresh time, and publish to Redis. A network error, a bad status, an empty or malformed body for either family leaves the stored list untouched and logs a warning (otherwise one missing family would drop trust for every peer of the other). Returns nothing. Blocking network calls. Tests: `test_09`, `test_10` and `test_10b` in `tests/test_trusted_ip_ranges.py` (the cron record also cites `test_cron_refresh_cloudflare_ip_ranges`).

6. The admin's own custom additions live in a separate settings field, never touched by that cron,
   so a manual edit is never silently clobbered by the next scheduled refresh.

### Settings fields for the ranges [@ANCHOR: COMM_trusted_ip_ranges_settings_fields]

On `res.config.settings` (`models/res_config_settings.py`): `cloudflare_trust_non_tunnel_peers` (Boolean, "Trust Cloudflare's Published IP Ranges", written through to the parameter `cloudflare.trust_non_tunnel_peers`), `cloudflare_trusted_ip_ranges_custom` (Char, "Additional Trusted IP Ranges", one CIDR range per line, written through to `cloudflare.trusted_ip_ranges_custom`), and two read-only computed Chars `cloudflare_trusted_ip_ranges_auto` (the fetched list, or a note that the built-in snapshot is in use) and `cloudflare_trusted_ip_ranges_last_refreshed` ("Never" before the first run). The custom field is a Char because settings screens reject Text; Char has no size limit unless set. `set_values()` publishes the list to Redis immediately after a save, so a change takes effect for the WSGI hook at once, not after the next cron tick or the 60 second hook cache. The custom list is never touched by the cron. Test: `test_13_settings_custom_field_is_config_parameter_backed_and_republishes`.

### The "Refresh now" button [@ANCHOR: COMM_action_refresh_cloudflare_trusted_ip_ranges]

`action_refresh_cloudflare_trusted_ip_ranges()` on `res.config.settings`, a `@public_rpc` button method. It calls `_cron_refresh_cloudflare_ip_ranges()` (the same logic, on demand, including the keep-the-old-list rules) and returns the client action `{"type": "ir.actions.client", "tag": "reload"}` so the settings page reloads and shows the new list and time. It has no explicit caller check of its own and works through the service account; the worst a caller can do is trigger a refresh from the two fixed URLs. To-do: confirm which RPC callers `@public_rpc` lets reach it. It raises nothing for a failed fetch (the old list stays, a warning is logged). Blocking network calls up to 20 seconds. Test: `test_14_refresh_action_invokes_the_cron_method_and_reloads`.

7. The merged, de-duplicated auto+custom list is what both the `env`-bearing peer-trust check
   and the env-less WSGI hook actually read; since the WSGI hook runs before any database
   connection exists, the merged list is also published to Redis on every cron refresh and every
   settings save, so it's never more than one tick stale.

### Peer trust with an environment [@ANCHOR: is_trusted_cf_peer_with_env]

`cloudflare.trusted_ip_utils._is_trusted_cf_peer(remote_addr)`, `@api.model`, for callers that have an `env` such as `get_request_context()`. `remote_addr` is the peer address string. It returns `True` when the parsed address is loopback (the Tunnel case, with IPv4-mapped IPv6 unmapped first) or falls inside any CIDR of `_get_effective_trusted_ip_ranges()`; `False` for an unparseable value, a missing one or a peer outside every range. It reads the database each call and never touches Redis (the env-less WSGI equivalent does the reverse). With the opt-in off, only loopback is trusted. No side effects. Tests: `test_04` to `test_08` in `tests/test_trusted_ip_ranges.py` (loopback, IPv4-mapped loopback, range peer with the mode on and off, untrusted address, custom range, unparseable address).

### The merged list [@ANCHOR: get_effective_trusted_ip_ranges]

`_get_effective_trusted_ip_ranges()` returns the merged, de-duplicated list of CIDR strings (in order: the auto list, then the custom ones). It is empty unless `cloudflare.trust_non_tunnel_peers` is `"True"`, which is the correct default for a Tunnel-only deployment, so a custom range alone never enables trust. The auto half is the stored fetched list, or the baked-in snapshot constants when that is empty (before the first successful cron run). Both halves go through `_parse_ranges`, which drops malformed tokens. Parameters are read as the trusted-IP service account. No arguments, no side effects, a list result. Callers: `_is_trusted_cf_peer` and `_publish_trusted_ip_ranges_to_redis`. Tests: `test_00` to `test_03b` in `tests/test_trusted_ip_ranges.py`.

### Publish to Redis [@ANCHOR: publish_trusted_ip_ranges_to_redis]

`_publish_trusted_ip_ranges_to_redis()` writes the current effective list as JSON to the Redis key `cloudflare:trusted_ip_ranges` with a three day expiry (longer than the daily cron, so a freshly restarted worker is never left without a key between publishes). Called after every cron refresh, every settings save and every module load. With the opt-in off it publishes an empty list on purpose, which replaces a wider list an older version may have left in Redis, because the WSGI hook keeps the last list it read when a key is missing. A Redis failure is logged as a warning and never raised, so a settings save or cron tick cannot break; the WSGI hook then keeps its last known list. Returns nothing. Tests: `test_11` and `test_13`.

### Republish when the module loads [@ANCHOR: COMM_republish_trusted_ip_ranges_on_module_load]

`data/republish_trusted_ip_ranges.xml` is a plain (not `noupdate`) data file, so it runs on every install and every upgrade of the module, unlike `post_init_hook`, which fires only on a fresh install. It calls the model method `_publish_trusted_ip_ranges_to_redis` via a `<function>` tag. It exists because, before the allow-list became opt-in, Redis held Cloudflare's full ranges on every install, and the WSGI hook keeps the last list across a missing key, so an upgraded worker could keep serving that wider list; after an upgrade the current (usually empty) list replaces it. On later upgrades it publishes the same list a Settings save would, which is harmless. Test: `test_12_module_upgrade_republishes_the_current_list_to_redis`.

## Scenario: Rotating Cloudflare credentials invalidates the cross-worker cache

8. `website`'s Cloudflare credential fields (API token, zone/account id, Turnstile secret) feed a
   `@distributed_cache()`'d method with a 24h TTL, so a plain `website.write()` that changes one of
   them must explicitly bust that Redis-backed cache -- otherwise every OTHER Odoo worker (and a
   fresh `odoo shell`) would keep using the stale credential for up to a day.

### `website.write()` busts the credential cache [@ANCHOR: COMM_website_write_busts_credential_cache]

The `write(vals)` override on `website` (`models/website.py`). It runs the real write, and when `vals` contains any of `cloudflare_api_token`, `cloudflare_api_token_crypt`, `cloudflare_zone_id`, `cloudflare_account_id`, `cloudflare_turnstile_secret` or `cloudflare_turnstile_secret_crypt` it calls `notify_model_invalidation(env, "website")`. That clears the local and Redis cache of the website model after the transaction commits and sends a Postgres notify so every other worker drops its copy; the `@distributed_cache()` on `_get_cloudflare_credentials` has a 24 hour lifetime, so without this a rotated credential would be ignored for up to a day. An unrelated write, such as renaming the website, does not invalidate anything. It then calls `_register_secrets_with_canary(vals)` and returns the result of `super().write()`. Test: `test_credential_write_busts_distributed_cache` in `tests/test_multi_website.py`.

## Tests

### `test_01_trusted_loopback_cf_visitor_https_sets_wsgi_url_scheme` [@ANCHOR: test_wsgi_proxy_scheme_fix_sets_https_for_trusted_cf_visitor]

Calls the patched entry point with a fake WSGI environ whose `REMOTE_ADDR` is loopback, `HTTP_CF_VISITOR` is `{"scheme":"https"}` and `wsgi.url_scheme` is `http`, with the original application replaced by a mock, and asserts the environ then has `wsgi.url_scheme == "https"` and `HTTP_X_FORWARDED_PROTO == "https"`, and that the original was called once with the same environ. It is the one case where the scheme is applied and so the module's reason to exist; the following tests prove an untrusted peer, a missing header and malformed JSON change nothing.

### `test_00_effective_ranges_are_empty_by_default_tunnel_only_deployment` [@ANCHOR: test_trusted_ip_ranges]

The first test of `tests/test_trusted_ip_ranges.py`, the named test for the whole merged-ranges feature. With the auto and custom lists emptied and the opt-in unset, it asserts that the effective list is empty and holds neither `173.245.48.0/20` nor `2400:cb00::/32`. It proves that a Tunnel-only deployment trusts no Cloudflare range by default. The tests after it cover the opt-in turning the snapshot on, custom additions, malformed ranges, peers, the refresh cron, and Redis publishing.

### `test_credential_write_busts_distributed_cache` [@ANCHOR: COMM_test_credential_write_busts_distributed_cache]

Patches `notify_model_invalidation` in the website module, writes `cloudflare_zone_id` on a website and asserts the patch was called once with the model name `"website"`, then resets it, renames the website and asserts it was not called. It proves that a credential write invalidates the cross-worker cache, and that an unrelated write does not pay that cost.

**Status:** Verified by the three test entries above.
