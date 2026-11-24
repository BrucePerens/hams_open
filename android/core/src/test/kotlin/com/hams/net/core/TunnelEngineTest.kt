package com.hams.net.core

import uniffi.hams_net_ffi.WgSession
import uniffi.hams_net_ffi.wgPublicKey
import java.net.InetAddress
import java.net.InetSocketAddress
import java.util.Base64
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

fun v6(src: String, dst: String, payload: ByteArray): ByteArray =
    byteArrayOf(0x60, 0, 0, 0, 0, payload.size.toByte(), 17, 64) +
        InetAddress.getByName(src).address + InetAddress.getByName(dst).address + payload

class TunnelEngineTest {
    private val endpoint = InetSocketAddress("127.0.0.1", 51820)
    private val ours = ByteArray(32) { (it + 1).toByte() }
    private val theirs = ByteArray(32) { (it + 100).toByte() }
    private val remote = WgSession(theirs, wgPublicKey(ours), 7u)
    private val sent = mutableListOf<Pair<InetSocketAddress, ByteArray>>()
    private val written = mutableListOf<ByteArray>()
    private var record: PeerRecord? = PeerRecord("2001:db8:1230::/60", Base64.getEncoder().encodeToString(wgPublicKey(theirs)), "127.0.0.1:51820")

    private val engine = TunnelEngine(ours, object : Datagrams {
        override fun send(to: InetSocketAddress, data: ByteArray) { sent += to to data }
    }, object : PacketSink {
        override fun write(packet: ByteArray) { written += packet }
    })
    private val table = PeerTable({ record }, engine, LinkType.INTERNET, { 0L }).also { engine.table = it }

    private val mine = "2001:db8:1200::2"
    private val peerAddress = "2001:db8:1230::1"

    // Pumps datagrams the engine sent to the remote and back until nothing moves; returns what the remote decrypted.
    private fun pump(): List<ByteArray> {
        val got = mutableListOf<ByteArray>()
        repeat(6) {
            val out = sent.toList(); sent.clear()
            for ((_, d) in out) {
                val r = remote.decapsulate(d)
                r.packet?.let(got::add)
                for (reply in r.replies) engine.fromNetwork(endpoint, reply)
            }
        }
        return got
    }

    @Test fun theFirstPacketMakesThePeerAndStartsTheHandshake() {
        val packet = v6(mine, peerAddress, "hello".toByteArray())
        engine.fromTun(packet)
        assertEquals(endpoint, sent[0].first)
        assertEquals(148, sent[0].second.size)
        val got = pump()
        assertEquals(1, got.size)
        assertContentEquals(packet, got[0])
    }

    @Test fun aPacketFromThePeerReachesTheInterfaceOnlyFromItsOwnPrefix() {
        engine.fromTun(v6(mine, peerAddress, "x".toByteArray()))
        pump()
        val good = v6(peerAddress, mine, "reply".toByteArray())
        engine.fromNetwork(endpoint, remote.encapsulate(good)[0])
        assertEquals(1, written.size)
        assertContentEquals(good, written[0])
        engine.fromNetwork(endpoint, remote.encapsulate(v6("2001:db8:9::1", mine, "spoof".toByteArray()))[0])
        assertEquals(1, written.size)
    }

    @Test fun aDatagramFromAnotherEndpointIsIgnored() {
        engine.fromTun(v6(mine, peerAddress, "x".toByteArray()))
        pump()
        engine.fromNetwork(InetSocketAddress("127.0.0.1", 9), remote.encapsulate(v6(peerAddress, mine, "x".toByteArray()))[0])
        assertTrue(written.isEmpty())
    }

    @Test fun noDirectoryRecordMeansNothingIsSent() {
        record = null
        engine.fromTun(v6(mine, peerAddress, "x".toByteArray()))
        assertTrue(sent.isEmpty())
    }

    @Test fun anEvictedPeerIsGone() {
        engine.fromTun(v6(mine, peerAddress, "x".toByteArray()))
        sent.clear()
        table.evictAll()
        engine.fromNetwork(endpoint, remote.encapsulate(v6(peerAddress, mine, "late".toByteArray()))[0])
        assertTrue(written.isEmpty())
        assertNotNull(Ipv6Packet.destination(v6(mine, peerAddress, ByteArray(0))))
    }
}
