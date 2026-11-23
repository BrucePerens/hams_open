package com.hams.net.core

import org.json.JSONException
import org.json.JSONObject
import uniffi.hams_net_ffi.PlatformCrypto

/**
 * Verifies a signed document of the deprecation channel (`{"payload": b64, "signature": b64}`, Ed25519 in usign's layout:
 * `"Ed"` + 8-byte key number + signature). Used for the status document; the list documents are verified in the Rust core.
 */
object SignedDocument {
    /** The payload JSON when `signature` verifies under `publicKeyLine` (usign public key, base64 of `"Ed"` + key number + 32-byte key). */
    fun verify(crypto: PlatformCrypto, publicKeyLine: String, payload: String, signature: String): JSONObject? {
        val key = crypto.base64Decode(publicKeyLine.trim())
        val sig = crypto.base64Decode(signature)
        val body = crypto.base64Decode(payload)
        if (key.size != 42 || sig.size != 74 || body.isEmpty()) return null
        if (key[0] != 'E'.code.toByte() || key[1] != 'd'.code.toByte() || sig[0] != key[0] || sig[1] != key[1]) return null
        if (!key.copyOfRange(2, 10).contentEquals(sig.copyOfRange(2, 10))) return null
        if (!crypto.ed25519Verify(key.copyOfRange(10, 42), sig.copyOfRange(10, 74), body)) return null
        return try {
            JSONObject(String(body, Charsets.UTF_8))
        } catch (e: JSONException) {
            null
        }
    }
}
