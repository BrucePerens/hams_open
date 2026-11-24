package com.hams.net.core

import uniffi.hams_net_ffi.WgSession
import uniffi.hams_net_ffi.wgPublicKey
import java.net.InetSocketAddress
import java.util.Base64

/** Where encrypted datagrams go (one protected UDP socket in the app). */
interface Datagrams {
    fun send(to: InetSocketAddress, data: ByteArray)
}

/** Where decrypted packets go (the VPN interface). */
interface PacketSink {
    fun write(packet: ByteArray)
}

/**
 * The userspace WireGuard side of the app: it is the [WgBackend] of the [PeerTable] and the packet hook. A packet read from the VPN
 * interface asks the table for the peer of its destination (which makes the peer on the first packet: the on-demand tunnel), is
 * encrypted by that peer's session and sent to its endpoint. A datagram from a peer's endpoint is decrypted and written to the
 * interface only when its source address lies in that peer's /60 (WireGuard's cryptokey routing); anything else is dropped.
 * `privateKey` is the 32-byte key of this device and stays inside this object and the native session.
 */
class TunnelEngine(private val privateKey: ByteArray, private val net: Datagrams, private val tun: PacketSink) : WgBackend {
    private class Peer(val session: WgSession, val endpoint: InetSocketAddress, val network: Ipv6Prefix)

    private val peers = mutableMapOf<String, Peer>()
    private var nextIndex = 1

    /** Set once, right after the table is built from this engine. */
    lateinit var table: PeerTable

    val publicKey: ByteArray get() = wgPublicKey(privateKey)

    @Synchronized
    override fun addPeer(publicKey: String, endpoint: String, allowedIp: String) {
        val network = Ipv6Prefix.parse(allowedIp) ?: return
        val key = try {
            Base64.getDecoder().decode(publicKey)
        } catch (e: IllegalArgumentException) {
            return
        }
        val at = endpoint.lastIndexOf(':')
        val port = endpoint.substring(at + 1).toIntOrNull() ?: return
        if (at < 1 || port !in 1..65535) return
        val host = endpoint.substring(0, at).removeSurrounding("[", "]")
        val session = try {
            WgSession(privateKey, key, nextIndex++.toUInt())
        } catch (e: uniffi.hams_net_ffi.NetException) {
            return
        }
        peers[publicKey] = Peer(session, InetSocketAddress(host, port), network)
    }

    @Synchronized
    override fun removePeer(publicKey: String) {
        peers.remove(publicKey)
    }

    /** A packet read from the VPN interface. Dropped when no peer may be made for its destination. */
    fun fromTun(packet: ByteArray) {
        val destination = Ipv6Packet.destination(packet) ?: return
        val key = table.onPacket(destination) ?: return
        val peer = synchronized(this) { peers[key] } ?: return
        try {
            for (d in peer.session.encapsulate(packet)) net.send(peer.endpoint, d)
        } catch (e: uniffi.hams_net_ffi.NetException) {
            // a packet the session refuses is dropped
        }
    }

    /** A datagram received from `from`. */
    fun fromNetwork(from: InetSocketAddress, datagram: ByteArray) {
        val peer = synchronized(this) { peers.values.firstOrNull { it.endpoint == from } } ?: return
        try {
            val received = peer.session.decapsulate(datagram)
            for (d in received.replies) net.send(peer.endpoint, d)
            val packet = received.packet ?: return
            val source = Ipv6Packet.source(packet) ?: return
            if (peer.network.contains(source)) tun.write(packet)
        } catch (e: uniffi.hams_net_ffi.NetException) {
            // not a valid datagram from this peer
        }
    }

    private var ticks = 0

    /** About every 250 ms: handshake retries and keepalives; about every 10 s idle peers are dropped. */
    fun tick() {
        if (++ticks % 40 == 0) table.expire()
        val now = synchronized(this) { peers.values.toList() }
        for (peer in now) {
            try {
                for (d in peer.session.tick()) net.send(peer.endpoint, d)
            } catch (e: uniffi.hams_net_ffi.NetException) {
                // nothing to send
            }
        }
    }
}
