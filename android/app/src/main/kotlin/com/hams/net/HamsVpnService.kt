package com.hams.net

import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.ProxyInfo
import android.net.Uri
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import com.hams.net.core.Blocklist
import com.hams.net.core.DeprecationRuntime
import com.hams.net.core.DirectoryClient
import com.hams.net.core.HamRouting
import com.hams.net.core.HamResolver
import com.hams.net.core.Ipv6Prefix
import com.hams.net.core.JvmCrypto
import com.hams.net.core.LinkGuard
import com.hams.net.core.LinkType
import com.hams.net.core.LocalProxy
import com.hams.net.core.PacketSink
import com.hams.net.core.PeerTable
import com.hams.net.core.TunLoop
import com.hams.net.core.TunnelEngine
import com.hams.net.core.UdpDatagrams
import java.io.FileInputStream
import java.io.FileOutputStream

/**
 * The on-demand tunnel. Only the ham-network prefix is routed into the interface (HamRouting.vpnRoutes), so all other traffic never
 * enters it; `.ham` names and whitelisted sites reach the app's local proxy through a PAC (Android 10 and later). The first packet
 * (or the first proxy request) toward a member /60 makes the WireGuard peer ([PeerTable] over [TunnelEngine], userspace WireGuard in the
 * shared Rust core). Refuses to start without an enrolment or when the active network is not a plain internet link.
 */
class HamsVpnService : VpnService() {
    private var tun: ParcelFileDescriptor? = null
    private var deprecation: DeprecationRuntime? = null
    private var proxy: LocalProxy? = null
    private var udp: UdpDatagrams? = null
    private var loop: TunLoop? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val enrolment = EnrolmentStore.load(this)
        if (enrolment == null || activeLink() != LinkType.INTERNET || tun != null) {
            if (tun == null) stopSelf()
            return if (tun == null) START_NOT_STICKY else START_STICKY
        }
        val network = Ipv6Prefix.parse(enrolment.hamPrefix) ?: return START_NOT_STICKY.also { stopSelf() }
        val own = Ipv6Prefix.parse(enrolment.prefix60) ?: return START_NOT_STICKY.also { stopSelf() }
        val routing = HamRouting(network, WHITELIST)

        val datagrams = UdpDatagrams { socket -> protect(socket) }
        udp = datagrams
        val toTun = object : PacketSink {
            override fun write(packet: ByteArray) { loop?.sink?.write(packet) }
        }
        val engine = TunnelEngine(KeyVault.privateKey(this), datagrams, toTun)
        val blocklist = Blocklist()
        val directory = DirectoryClient(Portal::post)
        val table = PeerTable({ directory.lookup(it) }, engine, LinkType.INTERNET, { System.currentTimeMillis() / 1000 }, blocklist = blocklist)
        engine.table = table
        val localProxy = LocalProxy(routing, HamResolver(directory, table)).also { it.start() }
        proxy = localProxy
        proxyPort = localProxy.port

        val builder = Builder().setSession("Hams.com Net").addAddress(own.host(2), 128)
        for (route in routing.vpnRoutes()) {
            val (address, length) = route.split("/")
            builder.addRoute(address, length.toInt())
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setHttpProxy(ProxyInfo.buildPacProxy(Uri.parse("http://127.0.0.1:${localProxy.port}/proxy.pac")))
        }
        val fd = builder.establish() ?: return START_NOT_STICKY.also { stopSelf() }
        tun = fd
        val tunLoop = TunLoop(FileInputStream(fd.fileDescriptor), FileOutputStream(fd.fileDescriptor), engine)
        loop = tunLoop
        Thread(tunLoop::run, "hams-tun").start()
        Thread({ datagrams.receiveLoop(engine) }, "hams-udp").start()
        deprecation = DeprecationRuntime(enrolment, JvmCrypto, Portal::post, table, blocklist, own.host(2)).also { it.start() }
        return START_STICKY
    }

    private fun activeLink(): LinkType {
        val manager = getSystemService(ConnectivityManager::class.java)
        val caps = manager.getNetworkCapabilities(manager.activeNetwork) ?: return LinkType.RF
        val transports = mutableSetOf<String>()
        if (caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) transports += "wifi"
        if (caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) transports += "cellular"
        if (caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) transports += "ethernet"
        if (caps.hasTransport(NetworkCapabilities.TRANSPORT_BLUETOOTH)) transports += "bluetooth"
        return LinkGuard.classify(transports)
    }

    override fun onDestroy() {
        deprecation?.stop()
        loop?.stop()
        proxy?.stop()
        proxyPort = 0
        udp?.close()
        tun?.close()
        tun = null
        super.onDestroy()
    }

    companion object {
        val WHITELIST = setOf("hams.com")

        /** The local proxy port while the tunnel is up (read by the emulator test). */
        @Volatile var proxyPort = 0
    }
}
