package com.hams.net.core

import org.json.JSONException
import org.json.JSONObject

/** What the portal enrolment (`/api/ipv6/enroll/poll`, status success) hands the device that the deprecation channel needs. */
data class Enrolment(val token: String, val deprecationKey: String, val prefix60: String) {
    companion object {
        fun fromJson(json: String): Enrolment? = try {
            val o = JSONObject(json)
            val token = o.optString("token")
            val key = o.optString("deprecation_key")
            if (o.optString("status") != "success" || token.isEmpty() || key.isEmpty()) null
            else Enrolment(token, key, o.optString("prefix60"))
        } catch (e: JSONException) {
            null
        }
    }
}
