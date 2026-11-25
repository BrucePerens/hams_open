package com.hams.net.core

import org.json.JSONException
import org.json.JSONObject
import java.security.SecureRandom
import uniffi.hams_net_ffi.PlatformCrypto

/**
 * Trust in the enrolment config and directory replies (docs/proposals/IPV6_CONFIG_SIGNING.md in hams_com): hams.com signs them with the
 * deprecation key and echoes the request's nonce. The app carries a PINNED list of acceptable public keys (`hams_pinned_keys.txt`, filled at
 * release time) and verifies against it, never against a key inside a reply. An empty list keeps the old behaviour (callers warn once).
 */
object SignedReply {
    /** The key lines of a list text; comments and anything that is not base64 are skipped. */
    fun keysOf(text: String): List<String> =
        text.lines().map { it.trim() }.filter { l -> l.isNotEmpty() && l.all { it.isLetterOrDigit() && it.code < 128 || it in "+/=" } }

    /** The list the app ships. */
    fun shipped(): List<String> =
        SignedReply::class.java.getResourceAsStream("/hams_pinned_keys.txt")?.use { keysOf(it.readBytes().toString(Charsets.UTF_8)) } ?: emptyList()

    fun nonce(): String = ByteArray(16).also { SecureRandom().nextBytes(it) }.joinToString("") { "%02x".format(it) }

    fun warnUnpinned(who: String) =
        System.err.println("hams.net $who: no pinned key list: the reply is trusted on TLS alone")

    /**
     * The signed document in `reply` (`result.signed`) and the pinned key that verified it, when one of `pins` signed it, it has `kind` and it
     * echoes `nonce`; null otherwise.
     */
    fun verify(crypto: PlatformCrypto, pins: List<String>, reply: String, nonce: String, kind: String): Pair<JSONObject, String>? {
        val signed = try {
            JSONObject(reply).optJSONObject("result")?.optJSONObject("signed")
        } catch (e: JSONException) {
            null
        } ?: return null
        for (pin in pins) {
            val doc = SignedDocument.verify(crypto, pin, signed.optString("payload"), signed.optString("signature")) ?: continue
            return if (doc.optString("kind") == kind && doc.optString("nonce") == nonce) doc to pin else null
        }
        return null
    }
}
