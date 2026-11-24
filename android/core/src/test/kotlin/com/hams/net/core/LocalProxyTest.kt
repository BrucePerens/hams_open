package com.hams.net.core

import org.json.JSONObject
import java.net.ServerSocket
import java.net.Socket
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class LocalProxyTest {
    private class Wg : WgBackend {
        val added = mutableListOf<String>()
        override fun addPeer(publicKey: String, endpoint: String, allowedIp: String) { added += allowedIp }
        override fun removePeer(publicKey: String) {}
    }

    private val wg = Wg()
    private val lookups = mutableListOf<String>()
    private val post = { _: String, body: String ->
        lookups += JSONObject(body).getJSONObject("params").getString("query")
        JSONObject().put("result", JSONObject().put("status", "success").put("prefix60", "2001:db8:1230::/60")
            .put("public_key", "KEY").put("endpoint", "127.0.0.1:51820")).toString()
    }
    private val directory = DirectoryClient(post)
    private val table = PeerTable({ directory.lookup(it) }, wg, LinkType.INTERNET, { 0L })
    private val routing = HamRouting(Ipv6Prefix.parse("2001:db8:1200::/40")!!, setOf("localhost"))
    private val proxy = LocalProxy(routing, HamResolver(directory, table)).also { it.start() }

    private fun ask(request: String): String = Socket("127.0.0.1", proxy.port).use {
        it.soTimeout = 20_000
        it.getOutputStream().write(request.toByteArray())
        it.getInputStream().readNBytes(4096).toString(Charsets.ISO_8859_1)
    }

    @Test fun theResolverMakesThePeerBeforeAnyPacket() {
        val resolver = HamResolver(directory, table)
        assertEquals(java.net.InetAddress.getByName("2001:db8:1230::1"), java.net.InetAddress.getByName(resolver.resolve("w1aw.ham")))
        assertEquals(listOf("2001:db8:1230::/60"), wg.added)
        assertEquals(null, resolver.resolve("example.com"))
    }

    @Test fun otherSitesAreRefusedAndThePacServesOnlyHamAndWhitelist() {
        assertTrue(ask("CONNECT example.com:443 HTTP/1.1\r\n\r\n").startsWith("HTTP/1.1 403"))
        assertTrue(lookups.isEmpty() && wg.added.isEmpty())
        val pac = ask("GET /proxy.pac HTTP/1.1\r\n\r\n")
        assertTrue(pac.contains("dnsDomainIs(host, \".ham\")") && pac.contains("return \"DIRECT\""))
    }

    @Test fun aWhitelistedSiteIsReachedDirectly() {
        ServerSocket(0, 1, java.net.InetAddress.getByName("127.0.0.1")).use { origin ->
            Thread { origin.accept().use { s -> s.getOutputStream().write("pong".toByteArray()) } }.start()
            val reply = ask("CONNECT localhost:${origin.localPort} HTTP/1.1\r\n\r\n")
            assertTrue(reply.startsWith("HTTP/1.1 200") && reply.endsWith("pong"), reply)
        }
    }

    @Test fun aHamNameAsksTheDirectoryAndMakesThePeer() {
        ask("CONNECT w1aw.ham:80 HTTP/1.1\r\n\r\n") // the tunnel is not there in a unit test: the connect fails, the peer was still made
        assertTrue("w1aw.ham" in lookups)
        assertEquals(listOf("2001:db8:1230::/60"), wg.added)
    }
}
