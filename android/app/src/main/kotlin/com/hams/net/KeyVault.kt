package com.hams.net

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import uniffi.hams_net_ffi.wgPublicKey
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Secrets at rest: sealed with an AES-GCM key that lives in the Android Keystore and cannot be exported, stored in the app's private
 * preferences. The WireGuard private key is made here, never leaves the app (not in a log, not in a request) and is never stored in the clear.
 */
object KeyVault {
    private const val ALIAS = "hams_vault"

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).setKeySize(256).build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply { init(spec) }.generateKey()
    }

    private fun prefs(context: Context) = context.getSharedPreferences("vault", Context.MODE_PRIVATE)

    fun seal(context: Context, name: String, bytes: ByteArray) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val sealed = cipher.iv + cipher.doFinal(bytes)
        prefs(context).edit().putString(name, Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
    }

    fun open(context: Context, name: String): ByteArray? {
        val text = prefs(context).getString(name, null) ?: return null
        return try {
            val raw = Base64.decode(text, Base64.NO_WRAP)
            Cipher.getInstance("AES/GCM/NoPadding").run {
                init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, raw, 0, 12))
                doFinal(raw, 12, raw.size - 12)
            }
        } catch (e: java.security.GeneralSecurityException) {
            null
        }
    }

    /** This device's WireGuard private key; made on first use. */
    fun privateKey(context: Context): ByteArray =
        open(context, "wg_private") ?: ByteArray(32).also { SecureRandom().nextBytes(it); seal(context, "wg_private", it) }

    fun publicKeyBase64(context: Context): String =
        java.util.Base64.getEncoder().encodeToString(wgPublicKey(privateKey(context)))

}
