# Zero Sudo Daemon

This directory contains components related to the `zero_sudo` architecture, enabling secure, zero-trust IPC without requiring elevated privileges.

### Functions
- **Secure JSON-RPC Client**: Provides a standardized JSON-2 IPC client (`SecureJSONRPCClient`) for external daemons.
- **Authentication**: Ensures secure requests using HMAC-SHA256 signature authentication along with timestamps and nonces.
- **Self-Healing Key Management**: Automatically detects 401/403 Access Denied responses and reloads credentials from local `.env` files to seamlessly handle API key rotation.

### File Structure
- `json_rpc_client.py`: The secure client implementation.
