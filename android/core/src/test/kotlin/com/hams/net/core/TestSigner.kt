package com.hams.net.core

import org.json.JSONObject
import java.security.KeyPairGenerator
import java.security.Signature
import java.util.Base64

/** A test deprecation-list key: signs documents in usign's layout like hams.com does. */
class TestSigner(private val keynum: ByteArray = ByteArray(8) { it.toByte() }) {
    private val pair = KeyPairGenerator.getInstance("Ed25519").generateKeyPair()
    private val b64 = Base64.getEncoder()

    /** The usign public key line (`"Ed"` + key number + 32-byte key). */
    val publicKeyLine: String =
        b64.encodeToString("Ed".toByteArray() + keynum + pair.public.encoded.copyOfRange(12, 44))

    /** (payload, signature) as base64 for `body`. */
    fun sign(body: JSONObject): Pair<String, String> {
        val payload = body.toString().toByteArray()
        val raw = Signature.getInstance("Ed25519").run {
            initSign(pair.private)
            update(payload)
            sign()
        }
        return b64.encodeToString(payload) to b64.encodeToString("Ed".toByteArray() + keynum + raw)
    }

    fun envelope(body: JSONObject): String =
        sign(body).let { JSONObject().put("result", JSONObject().put("payload", it.first).put("signature", it.second)).toString() }
}
