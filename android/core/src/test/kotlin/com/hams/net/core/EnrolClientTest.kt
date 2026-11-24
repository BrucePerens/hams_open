package com.hams.net.core

import org.json.JSONObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class EnrolClientTest {
    private fun wrap(o: JSONObject) = JSONObject().put("result", o).toString()
    private val bodies = mutableListOf<String>()

    @Test fun startThenPollUntilApproved() {
        val replies = ArrayDeque(listOf(
            wrap(JSONObject().put("status", "success").put("device_code", "dc").put("user_code", "ABCD-EFGH").put("interval", 5)),
            wrap(JSONObject().put("status", "pending").put("message", "authorization_pending")),
            wrap(JSONObject().put("status", "success").put("token", "tok").put("deprecation_key", "dk").put("prefix60", "2001:db8:1230::/60").put("ham_prefix", "2001:db8:1200::/40")),
        ))
        val client = EnrolClient { _, body -> bodies += body; replies.removeFirst() }
        val start = client.start()!!
        assertEquals(EnrolStart("dc", "ABCD-EFGH", 5), start)
        assertEquals(EnrolPoll.Pending, client.poll("dc", "PUB"))
        val done = client.poll("dc", "PUB") as EnrolPoll.Done
        assertEquals(Enrolment("tok", "dk", "2001:db8:1230::/60", "2001:db8:1200::/40"), done.enrolment)
        assertTrue(bodies.last().contains("\"public_key\":\"PUB\"") && !bodies.last().contains("private"))
    }

    @Test fun refusalAndUnreachablePortal() {
        assertEquals(EnrolPoll.Refused, EnrolClient { _, _ -> wrap(JSONObject().put("status", "error")) }.poll("dc", "P"))
        assertEquals(EnrolPoll.Pending, EnrolClient { _, _ -> null }.poll("dc", "P"))
        assertNull(EnrolClient { _, _ -> null }.start())
    }
}
