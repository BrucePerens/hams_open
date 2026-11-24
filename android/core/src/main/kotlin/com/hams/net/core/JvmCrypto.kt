package com.hams.net.core

import org.bouncycastle.crypto.params.Ed25519PublicKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer
import uniffi.hams_net_ffi.PlatformCrypto
import java.util.Base64

/**
 * The platform primitives for the shared Rust core. Ed25519 is BouncyCastle's lightweight pure-Java verifier, so it works on
 * every Android version the app supports (the platform provider only has Ed25519 from Android 13). Any malformed input says "not verified".
 */
object JvmCrypto : PlatformCrypto {
    override fun ed25519Verify(key: ByteArray, signature: ByteArray, message: ByteArray): Boolean = try {
        key.size == 32 && signature.size == 64 &&
            Ed25519Signer().run {
                init(false, Ed25519PublicKeyParameters(key, 0))
                update(message, 0, message.size)
                verifySignature(signature)
            }
    } catch (e: RuntimeException) {
        false
    }

    override fun base64Decode(text: String): ByteArray = try {
        Base64.getDecoder().decode(text)
    } catch (e: IllegalArgumentException) {
        ByteArray(0)
    }
}
