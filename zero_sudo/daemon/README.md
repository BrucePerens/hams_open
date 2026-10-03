# Zero Sudo Daemon

This directory contains components related to the `zero_sudo` architecture, enabling secure, zero-trust IPC (inter-process communication) without requiring elevated privileges. Everything here is plain Python that must not import `odoo`, so standalone daemons can import it without Odoo on their Python path.

### Functions
- **Secure JSON-RPC Client**: Provides a standardized JSON-2 IPC client (`SecureJSONRPCClient`) for external daemons, matching Odoo's real JSON-2 wire protocol (`odoo.http.Json2Dispatcher`). JSON-2 is Odoo's `POST /json/2/<model>/<method>` route: the JSON body carries `ids` plus the method's own keyword arguments, and the database is named in the `X-Odoo-Database` header.
- **Authentication**: Sends a real `Authorization: Bearer <api_key>` header, checked server-side against `res.users.apikeys` (the route is declared `auth='bearer'`). (Corrected 2026-09-13: this previously described a home-grown HMAC-SHA256/timestamp/nonce scheme that no server-side code ever validated -- the client never actually authenticated against the real endpoint until fixed the same day, see the comment in `call()` in `daemon/json_rpc_client.py` for the full history.)
- **Credentials**: The login and API key are read from `ODOO_RPC_LOGIN=` and `ODOO_RPC_KEY=` lines in a local daemon_key_manager `.env` file. The constructor raises `FileNotFoundError` if the file is missing and `ValueError` if either line is absent.
- **Self-Healing Key Management**: Automatically detects a real HTTP 401/403 response and reloads credentials from the local `.env` file, retrying once with the freshly-reloaded key, to seamlessly handle API key rotation. An error in the final response is raised as `RuntimeError`.

### File Structure
- `json_rpc_client.py`: The secure client implementation.
- `ssrf_safe_fetch.py`: An SSRF-safe (Server-Side Request Forgery) HTTP(S) fetch helper, `urlopen_ssrf_safe()`. It resolves each hostname once, rejects any non-public address (loopback, private, link-local, and similar), re-checks every redirect target, and connects only to the address it validated, so a DNS answer that changes between the check and the connection cannot redirect the request. A rejected URL raises `SSRFValidationError`.
