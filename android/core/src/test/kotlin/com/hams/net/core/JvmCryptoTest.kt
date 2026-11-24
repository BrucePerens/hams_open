package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Ed25519 without the platform provider (Android before 13 has none): RFC 8032 test vectors and a JDK-made signature. */
class JvmCryptoTest {
    private fun hex(s: String) = ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

    private val key1 = hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
    private val sig1 = hex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b")

    @Test fun rfc8032VectorVerifiesAndATamperedOneDoesNot() {
        assertTrue(JvmCrypto.ed25519Verify(key1, sig1, ByteArray(0)))
        assertFalse(JvmCrypto.ed25519Verify(key1, sig1, byteArrayOf(0)))
        assertFalse(JvmCrypto.ed25519Verify(key1, sig1.copyOf().also { it[0] = (it[0] + 1).toByte() }, ByteArray(0)))
    }

    @Test fun malformedInputIsNotVerified() {
        assertFalse(JvmCrypto.ed25519Verify(ByteArray(31), sig1, ByteArray(0)))
        assertFalse(JvmCrypto.ed25519Verify(key1, ByteArray(63), ByteArray(0)))
        assertFalse(JvmCrypto.ed25519Verify(ByteArray(32) { 0xFF.toByte() }, sig1, ByteArray(0)))
    }

    @Test fun aJdkSignedDocumentVerifies() {
        val signer = TestSigner()
        val (payload, signature) = signer.sign(org.json.JSONObject().put("kind", "status"))
        assertTrue(SignedDocument.verify(JvmCrypto, signer.publicKeyLine, payload, signature) != null)
    }
}
