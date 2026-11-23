package com.hams.net.core

/** Classifies the underlying link from its transport names; anything not clearly an internet link is RF and refused. */
object LinkGuard {
    private val internet = setOf("wifi", "cellular", "ethernet")
    private val modifiers = setOf("validated", "vpn")

    fun classify(transports: Set<String>): LinkType {
        val real = transports - modifiers
        return if (real.isNotEmpty() && real.all { it in internet }) LinkType.INTERNET else LinkType.RF
    }
}

/** The private key lives only here (on a device it comes from the Keystore); it is never in toString or anything sent. */
class KeyHolder(private val privateKey: String, val publicKey: String) {
    fun privateKeyForTunnel(): String = privateKey
    override fun toString() = "KeyHolder(publicKey=$publicKey)"
}

object CaGuidance {
    fun text(): String = """
        Hams.com Net uses its own certificate authority for .ham sites. To use them in a browser, install the Hams.com Net CA as a user certificate: Settings, Security, Encryption and credentials, Install a certificate, CA certificate.

        Chrome and Firefox on Android trust user-installed CAs, so .ham sites open there without warnings.
        Most other apps (banking, messaging, many in-app browsers) will not trust a user CA; they cannot open .ham sites over HTTPS. That is expected: only .ham, ham-network and whitelisted traffic is sent through the proxy, and everything else on your phone is untouched.
    """.trimIndent()
}
