package com.hams.net.core

import uniffi.hams_net_ffi.ListChange
import uniffi.hams_net_ffi.ListedKey

/** What the signed deprecation list and a signed `disabled` status forbid: no peer with a listed key, no traffic toward a listed prefix. */
class Blocklist {
    private val keys = mutableMapOf<String, List<Ipv6Prefix>>()
    private val certs = mutableSetOf<String>()

    /** True while a signed status says this node is disabled: nothing is routed to any peer. */
    @Volatile var disabled = false

    @Synchronized fun block(key: String, prefixes: List<Ipv6Prefix>) { keys[key] = prefixes }
    @Synchronized fun lift(key: String) { keys.remove(key) }
    @Synchronized fun blocksKey(key: String) = key in keys
    @Synchronized fun blocksDestination(address: String) = keys.values.any { list -> list.any { it.contains(address) } }

    /** A revoked member certificate (`cert:ISSUER:SERIAL` in the list); the TLS side refuses it through [isCertRevoked]. */
    @Synchronized fun revokeCert(issuer: String, serial: String) { certs += "$issuer:$serial" }
    @Synchronized fun unrevokeCert(issuer: String, serial: String) { certs -= "$issuer:$serial" }
    @Synchronized fun isCertRevoked(issuer: String, serial: String) = "$issuer:$serial" in certs
}

/** Enforces what the deprecation channel returns (poll or gossip): evict and block `added`, lift `removed`, stop on a signed disable. */
class DeprecationEnforcer(private val table: PeerTable, val blocklist: Blocklist) {
    fun apply(change: ListChange) {
        for (e in change.removed) listed(e, lift = true)
        for (e in change.added) listed(e, lift = false)
    }

    fun disable() {
        blocklist.disabled = true
        table.evictAll()
    }

    fun enable() {
        blocklist.disabled = false
    }

    private fun listed(e: ListedKey, lift: Boolean) {
        val cert = e.key.split(":")
        if (cert.size == 3 && cert[0] == "cert") {
            if (lift) blocklist.unrevokeCert(cert[1], cert[2]) else blocklist.revokeCert(cert[1], cert[2])
        } else if (lift) {
            blocklist.lift(e.key)
        } else {
            blocklist.block(e.key, e.prefixes.mapNotNull(Ipv6Prefix::parse))
            table.evict(e.key)
        }
    }
}
