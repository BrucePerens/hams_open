package com.hams.net.core

import org.json.JSONException
import org.json.JSONObject
import uniffi.hams_net_ffi.PlatformCrypto

/**
 * The members-only directory (`/api/ipv6/directory/lookup`). `post(path, jsonBody)` returns the response body or null. With a pinned key list
 * the request carries a nonce and only an answer signed by a pinned key for it counts (anything else is no record, so no peer is made); with an
 * empty list the old unsigned answer is used and a warning is printed once.
 */
class DirectoryClient(
    private val post: (String, String) -> String?,
    private val pins: List<String>,
    private val crypto: PlatformCrypto,
) {
    constructor(post: (String, String) -> String?) : this(post, SignedReply.shipped(), JvmCrypto)

    init {
        if (pins.isEmpty()) SignedReply.warnUnpinned("directory")
    }

    fun lookup(query: String): PeerRecord? {
        val params = JSONObject().put("query", query)
        val nonce = if (pins.isEmpty()) null else SignedReply.nonce().also { params.put("nonce", it) }
        val body = JSONObject()
            .put("jsonrpc", "2.0")
            .put("method", "call")
            .put("params", params)
            .toString()
        val reply = post("/api/ipv6/directory/lookup", body) ?: return null
        return try {
            val result = if (nonce == null) JSONObject(reply).optJSONObject("result") ?: return null
            else SignedReply.verify(crypto, pins, reply, nonce, "directory")?.first ?: return null
            if (result.optString("status") != "success") return null
            val prefix = result.optString("prefix60")
            val key = result.optString("public_key")
            val endpoint = result.optString("endpoint")
            if (prefix.isEmpty() || key.isEmpty() || endpoint.isEmpty()) null else PeerRecord(prefix, key, endpoint)
        } catch (e: JSONException) {
            null
        }
    }
}
