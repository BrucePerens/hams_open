package com.hams.net

import org.json.JSONObject
import java.net.InetAddress
import java.net.ServerSocket

/** A stand-in for hams.com on loopback: device enrolment (one "pending" first) and the members-only directory. */
class StubPortal(private val peerPublicKey: String, private val peerEndpoint: String) {
    private val server = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
    val base get() = "http://127.0.0.1:${server.localPort}"
    val seen = java.util.concurrent.CopyOnWriteArrayList<String>()
    @Volatile var enrolledKey: String? = null
    private var polls = 0

    val deprecationKey: String = java.util.Base64.getEncoder().encodeToString("Ed".toByteArray() + ByteArray(8) + ByteArray(32) { 5 })

    init {
        Thread({
            while (!server.isClosed) {
                val s = try { server.accept() } catch (e: java.io.IOException) { break }
                Thread { s.use { serve(it) } }.start()
            }
        }, "stub-portal").apply { isDaemon = true }.start()
    }

    fun stop() = server.close()

    private fun serve(s: java.net.Socket) {
        val input = s.getInputStream()
        val head = StringBuilder()
        while (!head.endsWith("\r\n\r\n")) { val b = input.read(); if (b < 0) return; head.append(b.toChar()) }
        val path = head.lines()[0].split(" ")[1]
        val length = Regex("(?i)content-length: (\\d+)").find(head)?.groupValues?.get(1)?.toInt() ?: 0
        val body = String(input.readNBytes(length))
        seen += path + (JSONObject(body.ifEmpty { "{}" }).optJSONObject("params")?.optString("query")?.let { " " + it } ?: "")
        val params = JSONObject(body.ifEmpty { "{}" }).optJSONObject("params") ?: JSONObject()
        val result = when (path) {
            "/api/ipv6/enroll/start" -> JSONObject().put("status", "success").put("device_code", "dc1").put("user_code", "ABCD-EFGH").put("interval", 1)
            "/api/ipv6/enroll/poll" ->
                if (params.optString("device_code") != "dc1") JSONObject().put("status", "error")
                else if (polls++ == 0) JSONObject().put("status", "pending")
                else {
                    enrolledKey = params.optString("public_key")
                    JSONObject().put("status", "success").put("token", "device-token-1").put("deprecation_key", deprecationKey)
                        .put("prefix60", "2001:db8:1210::/60").put("ham_prefix", "2001:db8:1200::/40").put("ham_name", "n0call.ham")
                }
            "/api/ipv6/directory/lookup" ->
                JSONObject().put("status", "success").put("prefix60", "2001:db8:1230::/60").put("public_key", peerPublicKey).put("endpoint", peerEndpoint)
            else -> null
        }
        val payload = (if (result == null) "{}" else JSONObject().put("jsonrpc", "2.0").put("result", result).toString()).toByteArray()
        s.getOutputStream().write("HTTP/1.1 ${if (result == null) 404 else 200} X\r\nContent-Type: application/json\r\nContent-Length: ${payload.size}\r\nConnection: close\r\n\r\n".toByteArray() + payload)
    }
}
