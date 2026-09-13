# Zero Sudo Daemon

This directory contains components related to the `zero_sudo` architecture, enabling secure, zero-trust IPC without requiring elevated privileges.

### Functions
- **Secure JSON-RPC Client**: Provides a standardized JSON-2 IPC client (`SecureJSONRPCClient`) for external daemons, matching Odoo's real JSON-2 wire protocol (`odoo.http.Json2Dispatcher`).
- **Authentication**: Sends a real `Authorization: Bearer <api_key>` header, checked server-side against `res.users.apikeys` (the route is declared `auth='bearer'`). (Corrected 2026-09-13: this previously described a home-grown HMAC-SHA256/timestamp/nonce scheme that no server-side code ever validated -- the client never actually authenticated against the real endpoint until fixed the same day, see `daemon/json_rpc_client.py`'s own claim for the full history.)
- **Self-Healing Key Management**: Automatically detects a real HTTP 401/403 response and reloads credentials from the local `.env` file, retrying once with the freshly-reloaded key, to seamlessly handle API key rotation.

### File Structure
- `json_rpc_client.py`: The secure client implementation.
