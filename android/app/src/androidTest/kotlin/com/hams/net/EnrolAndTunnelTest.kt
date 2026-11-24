package com.hams.net

import android.content.Intent
import androidx.test.platform.app.InstrumentationRegistry
import com.hams.net.core.Ipv6Packet
import org.junit.After
import org.junit.Test
import uniffi.hams_net_ffi.WgSession
import uniffi.hams_net_ffi.wgPublicKey
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.Socket
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * On a real (emulated) Android: enrol against a stub portal, bring the tunnel up, and show that the first request toward a member
 * makes the WireGuard peer on demand (handshake initiation on the wire) and that other traffic is not touched.
 */
class EnrolAndTunnelTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val theirs = ByteArray(32) { (it + 100).toByte() }
    private val peerSocket = DatagramSocket(0, InetAddress.getByName("127.0.0.1")).apply { soTimeout = 20_000 }
    private val portal = StubPortal(java.util.Base64.getEncoder().encodeToString(wgPublicKey(theirs)), "127.0.0.1:${peerSocket.localPort}")

    @After fun stop() {
        context.stopService(Intent(context, HamsVpnService::class.java))
        portal.stop()
        peerSocket.close()
    }

    private fun shell(command: String) {
        instrumentation.uiAutomation.executeShellCommand(command).let { android.os.ParcelFileDescriptor.AutoCloseInputStream(it).use { s -> s.readBytes() } }
    }

    private fun proxyAsk(request: String): String = Socket("127.0.0.1", HamsVpnService.proxyPort).use {
        it.soTimeout = 15_000
        it.getOutputStream().write(request.toByteArray())
        it.getInputStream().readNBytes(4096).toString(Charsets.ISO_8859_1)
    }

    @Test fun enrolThenTheFirstRequestMakesTheTunnel() {
        Portal.base = portal.base
        // 1. Enrolment: the code is shown, the member (the stub) approves, the token is kept sealed, only the public key was sent.
        var shown = ""
        assertTrue(EnrolFlow(context, pause = {}).run { shown = it })
        assertEquals("ABCD-EFGH", shown)
        val enrolment = assertNotNull(EnrolmentStore.load(context))
        assertEquals("device-token-1", enrolment.token)
        assertEquals(KeyVault.publicKeyBase64(context), portal.enrolledKey)
        val privateKeyText = java.util.Base64.getEncoder().encodeToString(KeyVault.privateKey(context))
        val prefsDir = java.io.File(context.applicationInfo.dataDir, "shared_prefs")
        for (f in prefsDir.listFiles().orEmpty()) {
            val text = f.readText()
            assertTrue("device-token-1" !in text && privateKeyText !in text, "plaintext secret in ${f.name}")
        }

        // 2. The tunnel (the VPN consent is pre-approved through the shell).
        shell("appops set ${context.packageName} ACTIVATE_VPN allow")
        assertEquals(null, android.net.VpnService.prepare(context))
        context.startService(Intent(context, HamsVpnService::class.java))
        val deadline = System.currentTimeMillis() + 20_000
        while (HamsVpnService.proxyPort == 0 && System.currentTimeMillis() < deadline) Thread.sleep(200)
        assertTrue(HamsVpnService.proxyPort != 0, "tunnel did not come up")

        // 3. Other traffic is not touched: the PAC sends only .ham and the whitelist to the proxy, a stranger is refused.
        val pac = proxyAsk("GET /proxy.pac HTTP/1.1\r\n\r\n")
        assertTrue(pac.contains("dnsDomainIs(host, \".ham\")") && pac.contains("return \"DIRECT\""))
        assertTrue(proxyAsk("CONNECT example.com:443 HTTP/1.1\r\n\r\n").startsWith("HTTP/1.1 403"))
        assertTrue(portal.seen.none { "example" in it }, portal.seen.toString())

        // 4. A request for a member's .ham name: directory lookup, peer made, handshake initiation on the wire to the member's endpoint.
        Thread { runCatching { proxyAsk("CONNECT n1abc.ham:80 HTTP/1.1\r\n\r\n") } }.start()
        val first = DatagramPacket(ByteArray(2048), 2048).also { peerSocket.receive(it) }
        assertEquals(148, first.length)
        assertEquals(1, first.data[0].toInt())
        assertTrue("/api/ipv6/directory/lookup n1abc.ham" in portal.seen, portal.seen.toString())

        // 5. The peer answers the handshake and then receives the queued first packet: a TCP SYN toward the member's router address.
        val remote = WgSession(theirs, wgPublicKey(privateOfApp()), 9u)
        val reply = remote.decapsulate(first.data.copyOf(first.length))
        for (r in reply.replies) peerSocket.send(DatagramPacket(r, r.size, first.socketAddress))
        var packet: ByteArray? = reply.packet
        while (packet == null) {
            val d = DatagramPacket(ByteArray(2048), 2048).also { peerSocket.receive(it) }
            packet = remote.decapsulate(d.data.copyOf(d.length)).packet
        }
        assertEquals(InetAddress.getByName("2001:db8:1230::1"), InetAddress.getByName(Ipv6Packet.destination(packet)))
        assertEquals(6, packet[6].toInt())
    }

    // The app's own private key is needed only to build the test's remote end of the tunnel (it needs our public key).
    private fun privateOfApp(): ByteArray = KeyVault.privateKey(context)
}
