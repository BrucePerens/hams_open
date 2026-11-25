package com.hams.net.core

import org.json.JSONObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SignedReplyTest {
    private val signer = TestSigner()
    private fun reply(s: TestSigner, doc: JSONObject): String {
        val (payload, sig) = s.sign(doc)
        return JSONObject().put("result", JSONObject().put("status", "success").put("signed", JSONObject().put("payload", payload).put("signature", sig))).toString()
    }

    @Test fun theShippedListIsEmptyUntilAReleaseFillsItAndCommentsAreSkipped() {
        assertTrue(SignedReply.shipped().isEmpty())
        assertEquals(listOf("AAAA+/=="), SignedReply.keysOf("# c\n\nAAAA+/==\nnot a key!\n"))
    }

    @Test fun onlyAPinnedKeyForThisKindAndNonceVerifies() {
        val doc = JSONObject().put("kind", "config").put("nonce", "n1").put("x", 1)
        val r = reply(signer, doc)
        val got = SignedReply.verify(JvmCrypto, listOf(signer.publicKeyLine), r, "n1", "config")!!
        assertEquals(1, got.first.getInt("x"))
        assertEquals(signer.publicKeyLine, got.second)
        assertNull(SignedReply.verify(JvmCrypto, listOf(signer.publicKeyLine), r, "n2", "config"))
        assertNull(SignedReply.verify(JvmCrypto, listOf(signer.publicKeyLine), r, "n1", "status"))
        assertNull(SignedReply.verify(JvmCrypto, listOf(TestSigner().publicKeyLine), r, "n1", "config"))
        assertNull(SignedReply.verify(JvmCrypto, listOf(signer.publicKeyLine), """{"result":{"x":1}}""", "n1", "config"))
        assertNull(SignedReply.verify(JvmCrypto, listOf(signer.publicKeyLine), "not json", "n1", "config"))
        assertNotNull(SignedReply.verify(JvmCrypto, listOf(TestSigner().publicKeyLine, signer.publicKeyLine), r, "n1", "config"), "a rotation list accepts either key")
    }
}
