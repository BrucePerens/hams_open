package com.hams.net.core

import org.json.JSONException
import org.json.JSONObject

/** The members-only directory (`/api/ipv6/directory/lookup`). `post(path, jsonBody)` returns the response body or null. */
class DirectoryClient(private val post: (String, String) -> String?) {
    fun lookup(query: String): PeerRecord? {
        val body = JSONObject()
            .put("jsonrpc", "2.0")
            .put("method", "call")
            .put("params", JSONObject().put("query", query))
            .toString()
        val reply = post("/api/ipv6/directory/lookup", body) ?: return null
        return try {
            val result = JSONObject(reply).optJSONObject("result") ?: return null
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
