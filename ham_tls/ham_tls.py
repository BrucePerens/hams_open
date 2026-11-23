# SPDX-License-Identifier: AGPL-3.0-or-later
"""Integrity-only TLS for the ham side of the hams.com Net proxy
(docs/proposals/IPV6_MEMBER_NETWORK.md, 'The ham-legal SSL proxy'). Part 97 forbids obscuring the meaning of
traffic over RF, so ham links use TLS with a NULL cipher: the peer is authenticated by its certificate and every
record carries a MAC (tamper and replay are detected), but the payload stays readable. No custom crypto: this
configures the system TLS library (OpenSSL) and refuses anything that encrypts.
"""

import ssl

# Integrity-only suites: RFC 9150 (TLS 1.3) names and the TLS 1.2 OpenSSL name.
INTEGRITY_ONLY_SUITES = frozenset({"NULL-SHA256", "TLS_SHA256_SHA256", "TLS_SHA384_SHA384"})
# OpenSSL 3.5 does implement the RFC 9150 suites, but Python's ssl module cannot select TLS 1.3 suites (no SSL_CTX_set_ciphersuites),
# so only the TLS 1.2 suite is offered here; SECLEVEL=0 is required for NULL suites. The Rust ham_proxy uses the 9150 suites.
_OPENSSL_CIPHERS = "NULL-SHA256:@SECLEVEL=0"


class EncryptedLinkError(ssl.SSLError):
    """The negotiated suite encrypts the payload, which is not allowed on a ham link."""


# [@ANCHOR: ham_tls:server_context]
def server_context(certfile, keyfile):
    """Server context offering only integrity-only suites (TLS 1.2 NULL-SHA256)."""
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = context.maximum_version = ssl.TLSVersion.TLSv1_2
    context.set_ciphers(_OPENSSL_CIPHERS)
    context.load_cert_chain(certfile, keyfile)
    return context


# [@ANCHOR: ham_tls:client_context]
def client_context(cafile):
    """Client context that verifies the server certificate against `cafile` and its hostname."""
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.minimum_version = context.maximum_version = ssl.TLSVersion.TLSv1_2
    context.set_ciphers(_OPENSSL_CIPHERS)
    context.load_verify_locations(cafile)
    return context


# [@ANCHOR: ham_tls:require_integrity_only]
def require_integrity_only(tls_socket):
    """Raise EncryptedLinkError unless the connection negotiated an integrity-only suite; returns its name."""
    name = tls_socket.cipher()[0]
    if name not in INTEGRITY_ONLY_SUITES:
        raise EncryptedLinkError("encrypting suite on a ham link: %s" % name)
    return name


# [@ANCHOR: ham_tls:connect]
def connect(sock, context, server_hostname):
    """Handshake as client over `sock` and check the suite; closes the socket on refusal."""
    tls_socket = context.wrap_socket(sock, server_hostname=server_hostname)
    try:
        require_integrity_only(tls_socket)
    except EncryptedLinkError:
        tls_socket.close()
        raise
    return tls_socket
