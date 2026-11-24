package com.hams.net.core

import org.json.JSONArray
import org.json.JSONObject
import uniffi.hams_net_ffi.DeprecationList
import uniffi.hams_net_ffi.ListChange
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class DeprecationPollerTest {
    private val signer = TestSigner()

    private class Sink : DeprecationSink {
        val changes = mutableListOf<ListChange>()
        val disabled = mutableListOf<String>()
        var enabled = 0
        override fun onChange(change: ListChange) { changes += change }
        override fun onDisabled(reason: String) { disabled += reason }
        override fun onEnabled() { enabled++ }
    }

    private fun status(state: String, seq: Long, reason: String = "") =
        signer.envelope(JSONObject().put("kind", "status").put("state", state).put("reason", reason).put("list_seq", seq))

    private fun entry(key: String) = JSONObject().put("key", key).put("prefixes", JSONArray().put("2001:db8:5::/60"))

    private fun full(seq: Long, vararg keys: String) =
        signer.envelope(JSONObject().put("kind", "full").put("seq", seq).put("entries", JSONArray(keys.map { entry(it) })))

    private fun delta(from: Long, seq: Long, add: String) =
        signer.envelope(JSONObject().put("kind", "delta").put("from", from).put("seq", seq).put("add", JSONArray().put(entry(add))))

    private class Server(val replies: Map<String, MutableList<String?>>) {
        val asked = mutableListOf<String>()
        fun post(path: String, body: String): String? {
            asked += path + " " + JSONObject(body).getJSONObject("params").optLong("have_seq", -1)
            return replies[path]?.removeFirstOrNull()
        }
    }

    private fun poller(server: Server, sink: Sink, list: DeprecationList = DeprecationList(JvmCrypto, signer.publicKeyLine)) =
        DeprecationPoller(server::post, "tok", signer.publicKeyLine, JvmCrypto, list, sink)

    // Tests [@ANCHOR: hams_net_ffi_deprecation_apply]
    @Test
    fun aNewerListIsFetchedAsFullThenAsDelta() {
        val sink = Sink()
        val server = Server(mapOf(
            "/api/ipv6/status" to mutableListOf(status("ok", 1), status("ok", 2)),
            "/api/ipv6/deprecations" to mutableListOf(full(1, "KEYA"), delta(1, 2, "KEYB")),
        ))
        val p = poller(server, sink)
        assertFalse(p.poll())
        assertEquals(listOf("KEYA"), sink.changes.single().added.map { it.key })
        assertFalse(p.poll())
        assertEquals(listOf("KEYB"), sink.changes.last().added.map { it.key })
        assertTrue("/api/ipv6/deprecations 1" in server.asked)
    }

    @Test
    fun aDeltaThatDoesNotLineUpFallsBackToTheFullList() {
        val sink = Sink()
        val list = DeprecationList(JvmCrypto, signer.publicKeyLine)
        list.apply(signer.sign(JSONObject().put("kind", "full").put("seq", 1).put("entries", JSONArray())).first,
            signer.sign(JSONObject().put("kind", "full").put("seq", 1).put("entries", JSONArray())).second)
        val server = Server(mapOf(
            "/api/ipv6/status" to mutableListOf(status("ok", 5)),
            "/api/ipv6/deprecations" to mutableListOf(delta(3, 5, "KEYB"), full(5, "KEYB")),
        ))
        poller(server, sink, list).poll()
        assertEquals(5UL, list.seq())
        assertEquals(listOf("/api/ipv6/deprecations 1", "/api/ipv6/deprecations 0"), server.asked.drop(1))
    }

    // Fail open: nothing but a correctly signed document changes anything.
    @Test
    fun unsignedWronglySignedAndMissingRepliesChangeNothing() {
        val sink = Sink()
        val other = TestSigner()
        val server = Server(mapOf("/api/ipv6/status" to mutableListOf(
            null, "not json", """{"result":{"status":"refused"}}""", other.envelope(JSONObject().put("kind", "status").put("state", "disabled").put("list_seq", 9)),
        )))
        val p = poller(server, sink)
        repeat(4) { assertFalse(p.poll()) }
        assertTrue(sink.disabled.isEmpty() && sink.changes.isEmpty())
    }

    @Test
    fun aSignedDisabledSwitchesOffAndOkSwitchesOnAndAnOlderStatusIsAReplay() {
        val sink = Sink()
        val server = Server(mapOf("/api/ipv6/status" to mutableListOf(
            status("disabled", 4, "exploited"), status("ok", 3), status("ok", 4),
        )))
        val p = poller(server, sink)
        assertTrue(p.poll())
        assertEquals(listOf("exploited"), sink.disabled)
        assertTrue(p.poll()) // the older ok is a replay
        assertEquals(0, sink.enabled)
        p.poll()
        assertEquals(1, sink.enabled)
    }
}
