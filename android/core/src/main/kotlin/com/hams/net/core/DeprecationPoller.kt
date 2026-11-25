package com.hams.net.core

import org.json.JSONException
import org.json.JSONObject
import java.security.SecureRandom
import uniffi.hams_net_ffi.DeprecationList
import uniffi.hams_net_ffi.ListChange
import uniffi.hams_net_ffi.PlatformCrypto

/** Where the poller's results go: the app evicts and blocks `added`, lifts `removed`, and switches off or on. */
interface DeprecationSink {
    fun onChange(change: ListChange)
    fun onDisabled(reason: String)
    fun onEnabled()
}

/**
 * The router's status poll against hams.com (`/api/ipv6/status`, then `/api/ipv6/deprecations`), see
 * docs/proposals/IPV6_DEPRECATION_LIST.md in hams_com. Fails open: any missing, unsigned, badly signed or older reply changes
 * nothing. Only a correctly signed `disabled` for this device switches the node off. When the status names a newer list than
 * the held one, the delta since the held number is fetched, and the full list when the delta does not line up. `post(path, jsonBody)`
 * returns the response body or null.
 */
class DeprecationPoller(
    private val post: (String, String) -> String?,
    private val token: String,
    private val publicKeyLine: String,
    private val crypto: PlatformCrypto,
    private val list: DeprecationList,
    private val sink: DeprecationSink,
) {
    private var statusSeq = 0L
    private var disabled = false

    /** One poll. True when the node is currently disabled. */
    fun poll(): Boolean {
        // A fresh nonce per poll that the signed status must carry back: a recorded status (or one made for another router) is no answer.
        val nonce = ByteArray(16).also { SecureRandom().nextBytes(it) }.joinToString("") { "%02x".format(it) }
        val status = fetch("/api/ipv6/status", JSONObject().put("token", token).put("nonce", nonce))?.first ?: return disabled
        if (status.optString("kind") != "status" || status.optString("nonce") != nonce) return disabled
        val listSeq = status.optLong("list_seq", -1)
        if (listSeq < statusSeq) return disabled // an older status is a replay
        statusSeq = listSeq
        when (status.optString("state")) {
            "disabled" -> {
                disabled = true
                sink.onDisabled(status.optString("reason"))
                return true
            }
            "ok" -> if (disabled) {
                disabled = false
                sink.onEnabled()
            }
            else -> return disabled
        }
        if (listSeq.toULong() > list.seq()) fetchList()
        return disabled
    }

    private fun fetchList() {
        val have = list.seq()
        for (ask in listOf(have, 0UL).distinct()) {
            val params = JSONObject().put("token", token).put("have_seq", ask.toLong())
            val (_, envelope) = fetch("/api/ipv6/deprecations", params) ?: return
            val change = list.apply(envelope.first, envelope.second)
            if (change != null) {
                sink.onChange(change)
                return
            }
        }
    }

    /** The verified payload and the raw (payload, signature) envelope of a reply, or null. */
    private fun fetch(path: String, params: JSONObject): Pair<JSONObject, Pair<String, String>>? {
        val body = JSONObject().put("jsonrpc", "2.0").put("method", "call").put("params", params).toString()
        val reply = post(path, body) ?: return null
        return try {
            val result = JSONObject(reply).optJSONObject("result") ?: return null
            val payload = result.optString("payload")
            val signature = result.optString("signature")
            SignedDocument.verify(crypto, publicKeyLine, payload, signature)?.let { it to (payload to signature) }
        } catch (e: JSONException) {
            null
        }
    }
}
