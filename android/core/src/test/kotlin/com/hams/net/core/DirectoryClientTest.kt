package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DirectoryClientTest {
    @Test fun lookupPostsJsonRpcAndParsesRecord() {
        var path = ""
        var body = ""
        val client = DirectoryClient { p, b ->
            path = p; body = b
            """{"jsonrpc":"2.0","id":1,"result":{"status":"success","callsign":"K6BP","prefix60":"2001:db8:1230::/60",
               "public_key":"KEYA","endpoint":"198.51.100.7:51820","interest":["a"]}}"""
        }
        val r = client.lookup("2001:db8:1230::1")
        assertEquals(PeerRecord("2001:db8:1230::/60", "KEYA", "198.51.100.7:51820"), r)
        assertEquals("/api/ipv6/directory/lookup", path)
        assertTrue(body.contains("\"query\":\"2001:db8:1230::1\""))
    }

    @Test fun notFoundAndTransportFailureGiveNull() {
        assertNull(DirectoryClient { _, _ -> """{"result":{"status":"error","message":"not found"}}""" }.lookup("K6BP"))
        assertNull(DirectoryClient { _, _ -> null }.lookup("K6BP"))
        assertNull(DirectoryClient { _, _ -> "not json" }.lookup("K6BP"))
    }

    @Test fun successWithMissingKeyIsNull() {
        assertNull(DirectoryClient { _, _ -> """{"result":{"status":"success","prefix60":"2001:db8::/60"}}""" }.lookup("x"))
    }
}
