package com.hams.net

import android.content.Context
import com.hams.net.core.Enrolment

/** The portal over https: the JSON-RPC body in, the response body out, null on any failure. `base` is only changed by tests. */
object Portal {
    @Volatile var base = "https://hams.com"

    fun post(path: String, body: String): String? = try {
        val conn = java.net.URL(base + path).openConnection(java.net.Proxy.NO_PROXY) as java.net.HttpURLConnection
        conn.requestMethod = "POST"
        conn.connectTimeout = 10_000
        conn.readTimeout = 10_000
        conn.doOutput = true
        conn.setRequestProperty("Content-Type", "application/json")
        conn.outputStream.use { it.write(body.toByteArray()) }
        if (conn.responseCode == 200) conn.inputStream.use { it.readBytes().toString(Charsets.UTF_8) } else null
    } catch (e: java.io.IOException) {
        null
    }
}

/** The enrolment result: the device token is sealed by the [KeyVault]; the rest is not secret. */
object EnrolmentStore {
    fun save(context: Context, e: Enrolment) {
        KeyVault.seal(context, "device_token", e.token.toByteArray())
        context.getSharedPreferences("enrolment", Context.MODE_PRIVATE).edit()
            .putString("deprecation_key", e.deprecationKey).putString("prefix60", e.prefix60).putString("ham_prefix", e.hamPrefix).apply()
    }

    fun load(context: Context): Enrolment? {
        val p = context.getSharedPreferences("enrolment", Context.MODE_PRIVATE)
        val token = KeyVault.open(context, "device_token")?.toString(Charsets.UTF_8) ?: return null
        val key = p.getString("deprecation_key", null) ?: return null
        return Enrolment(token, key, p.getString("prefix60", "") ?: "", p.getString("ham_prefix", "") ?: "")
    }
}
