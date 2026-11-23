package com.hams.net.core

enum class Route { PROXY, DIRECT }

/** Only `.ham`, ham-network addresses and whitelisted sites (and their subdomains) use the proxy; everything else is DIRECT. */
class HamRouting(private val network: Ipv6Prefix, whitelist: Set<String>) {
    private val whitelist = whitelist.map { it.lowercase() }.toSet()

    fun decideHost(host: String): Route {
        val h = host.lowercase().trimEnd('.')
        if (h == "ham" || h.endsWith(".ham")) return Route.PROXY
        return if (whitelist.any { h == it || h.endsWith(".$it") }) Route.PROXY else Route.DIRECT
    }

    fun decideAddress(address: String): Route = if (network.contains(address)) Route.PROXY else Route.DIRECT

    /** The only routes the VpnService adds: other traffic never enters the tunnel. */
    fun vpnRoutes(): List<String> = listOf(network.toString())

    fun pac(proxy: String): String {
        val domains = listOf("ham") + whitelist.sorted()
        val tests = domains.joinToString(" || ") { "dnsDomainIs(host, \".$it\") || host == \"$it\"" }
        return "function FindProxyForURL(url, host) { if ($tests) { return \"PROXY $proxy\"; } return \"DIRECT\"; }\n"
    }
}
