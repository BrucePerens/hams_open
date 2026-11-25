package com.hams.net.core

import org.json.JSONException
import org.json.JSONObject
import uniffi.hams_net_ffi.PlatformCrypto

/** The code a member approves at hams.com/my/ipv6, and the secret to poll with. */
data class EnrolStart(val deviceCode: String, val userCode: String, val intervalSeconds: Int)

sealed class EnrolPoll {
    object Pending : EnrolPoll()
    object Refused : EnrolPoll()
    data class Done(val enrolment: Enrolment) : EnrolPoll()
}

/**
 * Device enrolment (`/api/ipv6/enroll/start`, `/poll`): no password ever leaves the member's browser; the app shows the code,
 * sends only its WireGuard public key and keeps the device token it is given. `post(path, jsonBody)` returns the body or null.
 */
class EnrolClient(
    private val post: (String, String) -> String?,
    private val pins: List<String>,
    private val crypto: PlatformCrypto,
) {
    constructor(post: (String, String) -> String?) : this(post, SignedReply.shipped(), JvmCrypto)

    init {
        if (pins.isEmpty()) SignedReply.warnUnpinned("enrolment")
    }

    fun start(): EnrolStart? {
        val r = call("/api/ipv6/enroll/start", JSONObject()) ?: return null
        val device = r.optString("device_code")
        val user = r.optString("user_code")
        return if (r.optString("status") == "success" && device.isNotEmpty() && user.isNotEmpty()) EnrolStart(device, user, r.optInt("interval", 5)) else null
    }

    /** Pending while the member has not approved or the portal is unreachable (ask again later); Refused for anything else that is not a success. */
    fun poll(deviceCode: String, publicKeyBase64: String): EnrolPoll {
        val params = JSONObject().put("device_code", deviceCode).put("public_key", publicKeyBase64)
        val r = call("/api/ipv6/enroll/poll", params) ?: return EnrolPoll.Pending
        return when (r.optString("status")) {
            "pending" -> EnrolPoll.Pending
            "success" -> (if (pins.isEmpty()) Enrolment.fromJson(r.toString()) else signedConfig(r.optString("token")))?.let { EnrolPoll.Done(it) } ?: EnrolPoll.Refused
            else -> EnrolPoll.Refused
        }
    }

    /**
     * With a pinned key list the poll's own (unsigned) configuration is not used: the device token it carries fetches the configuration signed for a
     * fresh nonce (docs/proposals/IPV6_CONFIG_SIGNING.md), and the deprecation key kept is the pinned key that signed it, never the one in the reply.
     */
    private fun signedConfig(token: String): Enrolment? {
        if (token.isEmpty()) return null
        val nonce = SignedReply.nonce()
        val body = JSONObject().put("jsonrpc", "2.0").put("method", "call").put("params", JSONObject().put("token", token).put("nonce", nonce)).toString()
        val reply = post("/api/ipv6/enroll/config", body) ?: return null
        val (doc, key) = SignedReply.verify(crypto, pins, reply, nonce, "config") ?: return null
        return Enrolment.fromJson(doc.put("token", token).put("deprecation_key", key).toString())
    }

    private fun call(path: String, params: JSONObject): JSONObject? {
        val body = JSONObject().put("jsonrpc", "2.0").put("method", "call").put("params", params).toString()
        val reply = post(path, body) ?: return null
        return try { JSONObject(reply).optJSONObject("result") } catch (e: JSONException) { null }
    }
}
