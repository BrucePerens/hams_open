package com.hams.net

import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.VpnService
import android.content.Intent
import android.os.ParcelFileDescriptor
import com.hams.net.core.Blocklist
import com.hams.net.core.DeprecationRuntime
import com.hams.net.core.DirectoryClient
import com.hams.net.core.Enrolment
import com.hams.net.core.HamRouting
import com.hams.net.core.JvmCrypto
import com.hams.net.core.PeerTable
import com.hams.net.core.WgBackend
import com.hams.net.core.Ipv6Prefix
import com.hams.net.core.LinkGuard
import com.hams.net.core.LinkType

/** The portal over https (hams.com): the JSON-RPC body in, the response body out, null on any failure. */
object Portal {
    fun post(path: String, body: String): String? = try {
        val conn = java.net.URL("https://hams.com$path").openConnection() as java.net.HttpURLConnection
        conn.requestMethod = "POST"
        conn.connectTimeout = 10_000
        conn.readTimeout = 10_000
        conn.doOutput = true
        conn.setRequestProperty("Content-Type", "application/json")
        conn.outputStream.use { it.write(body.toByteArray()) }
        if (conn.responseCode == 200) conn.inputStream.use { it.readBytes().toString(Charsets.UTF_8) } else null
    } catch (e: java.io.IOException) {
        null
    }
}

/** Where the enrolment result is kept: the token and the deprecation key, in the app's private preferences. */
object EnrolmentStore {
    fun save(context: android.content.Context, e: Enrolment) {
        context.getSharedPreferences("enrolment", 0).edit()
            .putString("token", e.token).putString("deprecation_key", e.deprecationKey).putString("prefix60", e.prefix60).apply()
    }

    fun load(context: android.content.Context): Enrolment? {
        val p = context.getSharedPreferences("enrolment", 0)
        val token = p.getString("token", null) ?: return null
        val key = p.getString("deprecation_key", null) ?: return null
        return Enrolment(token, key, p.getString("prefix60", "") ?: "")
    }
}

/**
 * The WireGuard peers of the tunnel. The GoBackend-backed implementation is the remaining device-side step (it needs a device or
 * KVM emulator); until then the peer set is only tracked, which is all eviction needs.
 */
object TunnelPeers : WgBackend {
    val peers = java.util.concurrent.ConcurrentHashMap<String, String>()
    override fun addPeer(publicKey: String, endpoint: String, allowedIp: String) { peers[publicKey] = endpoint }
    override fun removePeer(publicKey: String) { peers.remove(publicKey) }
}

/**
 * Only the ham-network prefix is routed into the tunnel (HamRouting.vpnRoutes), so all other traffic never enters it.
 * Refuses to start when the active network is not a plain internet link. The WireGuard peers are made on demand by PeerTable
 * through a GoBackend-backed WgBackend (wiring to the portal enrolment is not in this skeleton).
 */
class HamsVpnService : VpnService() {
    private var tun: ParcelFileDescriptor? = null
    private var deprecation: DeprecationRuntime? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (activeLink() != LinkType.INTERNET) {
            stopSelf()
            return START_NOT_STICKY
        }
        val routing = HamRouting(Ipv6Prefix.parse(HAM_NETWORK)!!, WHITELIST)
        val builder = Builder().setSession("Hams.com Net").addAddress(LOCAL_ADDRESS, 128)
        for (route in routing.vpnRoutes()) {
            val (address, length) = route.split("/")
            builder.addRoute(address, length.toInt())
        }
        tun = builder.establish()
        startDeprecationChannel()
        return START_STICKY
    }

    // The status poll, gossip and enforcement need the enrolment (device token and deprecation_key); without it nothing is routed to peers either.
    private fun startDeprecationChannel() {
        val enrolment = EnrolmentStore.load(this) ?: return
        val blocklist = Blocklist()
        val directory = DirectoryClient(Portal::post)
        val table = PeerTable({ directory.lookup(it) }, TunnelPeers, LinkType.INTERNET, { System.currentTimeMillis() / 1000 }, blocklist = blocklist)
        deprecation = DeprecationRuntime(enrolment, JvmCrypto, Portal::post, table, blocklist, LOCAL_ADDRESS).also { it.start() }
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
        tun?.close()
        super.onDestroy()
    }

    companion object {
        // Placeholders until the ARIN allocation and the member's own /128 come from enrolment.
        const val HAM_NETWORK = "2001:db8:1200::/40"
        const val LOCAL_ADDRESS = "2001:db8:1200::1"
        val WHITELIST = setOf("hams.com")
    }
}
