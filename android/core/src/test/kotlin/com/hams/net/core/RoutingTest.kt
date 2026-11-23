package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RoutingTest {
    private val routing = HamRouting(Ipv6Prefix.parse("2001:db8:1200::/40")!!, setOf("hams.com"))

    @Test fun onlyHamNetworkAndWhitelistedGoThroughProxy() {
        assertEquals(Route.PROXY, routing.decideHost("k6bp.ham"))
        assertEquals(Route.PROXY, routing.decideHost("Shack.K6BP.HAM"))
        assertEquals(Route.PROXY, routing.decideHost("hams.com"))
        assertEquals(Route.PROXY, routing.decideHost("www.hams.com"))
        assertEquals(Route.DIRECT, routing.decideHost("example.com"))
        assertEquals(Route.DIRECT, routing.decideHost("evilhams.com"))
        assertEquals(Route.DIRECT, routing.decideHost("hams.com.evil.example"))
        assertEquals(Route.DIRECT, routing.decideHost("notham"))
    }

    @Test fun addressesInTheHamPrefixGoThroughProxy() {
        assertEquals(Route.PROXY, routing.decideAddress("2001:db8:1230::1"))
        assertEquals(Route.DIRECT, routing.decideAddress("2001:db9::1"))
        assertEquals(Route.DIRECT, routing.decideAddress("93.184.216.34"))
    }

    @Test fun vpnRoutesCoverOnlyTheHamPrefix() {
        assertEquals(listOf("2001:db8:1200::/40"), routing.vpnRoutes())
    }

    @Test fun pacSendsOtherTrafficDirect() {
        val pac = routing.pac("127.0.0.1:8118")
        assertTrue(pac.contains("dnsDomainIs(host, \".ham\")"))
        assertTrue(pac.contains("dnsDomainIs(host, \".hams.com\")"))
        assertTrue(pac.contains("PROXY 127.0.0.1:8118"))
        assertTrue(pac.trimEnd().endsWith("return \"DIRECT\"; }"))
        assertFalse(pac.contains("example.com"))
    }
}
