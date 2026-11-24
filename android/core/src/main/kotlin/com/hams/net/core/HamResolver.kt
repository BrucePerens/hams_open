package com.hams.net.core

/**
 * The `.ham` resolver: a name is looked up in the members-only directory and answers the member router's address (`::1` of the /60).
 * Resolving also asks the peer table for that address, which is what makes the WireGuard peer before the first packet.
 */
class HamResolver(private val directory: DirectoryClient, private val table: PeerTable) {
    fun resolve(name: String): String? {
        val h = name.lowercase().trimEnd('.')
        if (!h.endsWith(".ham")) return null
        val network = directory.lookup(h)?.let { Ipv6Prefix.parse(it.prefix60) } ?: return null
        val address = network.host()
        return if (table.onPacket(address) != null) address else null
    }
}
