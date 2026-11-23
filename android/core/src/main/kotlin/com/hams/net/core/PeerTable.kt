package com.hams.net.core

const val IDLE_SECONDS = 600L
const val REFUSED_SECONDS = 60L

data class PeerRecord(val prefix60: String, val publicKey: String, val endpoint: String)

/** The WireGuard side: the app's GoBackend tunnel in production, a fake in tests. */
interface WgBackend {
    fun addPeer(publicKey: String, endpoint: String, allowedIp: String)
    fun removePeer(publicKey: String)
}

enum class LinkType { INTERNET, RF }

/**
 * On-demand WireGuard peers, a port of ham_ipv6net/client/peers.py. The first packet toward a member /60 asks the directory
 * for that member's key and endpoint and adds a peer; a peer idle for ten minutes is removed. Never built on an RF link
 * (47 CFR 97.113(a)(4): WireGuard encrypts). `clock` returns seconds.
 */
class PeerTable(
    private val lookup: (String) -> PeerRecord?,
    private val wg: WgBackend,
    link: LinkType,
    private val clock: () -> Long,
    private val idle: Long = IDLE_SECONDS,
) {
    private class Entry(val network: Ipv6Prefix, var last: Long)

    private val peers = mutableMapOf<String, Entry>()
    private val refused = mutableMapOf<String, Long>()

    init {
        require(link == LinkType.INTERNET) { "a WireGuard peer is only made on an internet link, not on $link" }
    }

    /** The peer's public key for traffic toward `destination`, or null when the directory has no current record. */
    fun onPacket(destination: String): String? {
        val now = clock()
        for ((key, entry) in peers) {
            if (entry.network.contains(destination)) {
                entry.last = now
                return key
            }
        }
        val last = refused[destination]
        if (last != null && now - last < REFUSED_SECONDS) return null
        val record = lookup(destination)
        val network = record?.let { Ipv6Prefix.parse(it.prefix60) }
        if (record == null || network == null || !network.contains(destination)) {
            refused[destination] = now
            return null
        }
        wg.addPeer(record.publicKey, record.endpoint, record.prefix60)
        peers[record.publicKey] = Entry(network, now)
        return record.publicKey
    }

    /** Removes every peer idle for longer than the idle time; returns their public keys. */
    fun expire(): List<String> {
        val now = clock()
        val gone = peers.filter { now - it.value.last > idle }.keys.toList()
        for (key in gone) {
            wg.removePeer(key)
            peers.remove(key)
        }
        return gone
    }
}
