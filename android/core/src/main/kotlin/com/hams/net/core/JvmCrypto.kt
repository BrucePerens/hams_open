package com.hams.net.core

import uniffi.hams_net_ffi.PlatformCrypto
import java.security.KeyFactory
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import java.util.Base64

/**
 * The platform primitives for the shared Rust core, from the JDK/Android provider (Ed25519 needs Android 13+ or a JDK 15+;
 * on an older system every check says "not verified", which fails open: nothing is ever applied unverified).
 */
object JvmCrypto : PlatformCrypto {
    // DER prefix of an X.509 SubjectPublicKeyInfo holding a raw 32-byte Ed25519 key.
    private val SPKI = byteArrayOf(0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00)

    override fun ed25519Verify(key: ByteArray, signature: ByteArray, message: ByteArray): Boolean = try {
        if (key.size != 32 || signature.size != 64) {
            false
        } else {
            val pub = KeyFactory.getInstance("Ed25519").generatePublic(X509EncodedKeySpec(SPKI + key))
            Signature.getInstance("Ed25519").run {
                initVerify(pub)
                update(message)
                verify(signature)
            }
        }
    } catch (e: java.security.GeneralSecurityException) {
        false
    }

    override fun base64Decode(text: String): ByteArray = try {
        Base64.getDecoder().decode(text)
    } catch (e: IllegalArgumentException) {
        ByteArray(0)
    }
}
