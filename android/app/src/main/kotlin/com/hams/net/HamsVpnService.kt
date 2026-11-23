package com.hams.net

import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.VpnService
import android.content.Intent
import android.os.ParcelFileDescriptor
import com.hams.net.core.HamRouting
import com.hams.net.core.Ipv6Prefix
import com.hams.net.core.LinkGuard
import com.hams.net.core.LinkType

/**
 * Only the ham-network prefix is routed into the tunnel (HamRouting.vpnRoutes), so all other traffic never enters it.
 * Refuses to start when the active network is not a plain internet link. The WireGuard peers are made on demand by PeerTable
 * through a GoBackend-backed WgBackend (wiring to the portal enrolment is not in this skeleton).
 */
class HamsVpnService : VpnService() {
    private var tun: ParcelFileDescriptor? = null

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
