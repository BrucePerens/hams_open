package com.hams.net.core

import java.net.Inet6Address
import java.net.InetAddress

/** An IPv6 network such as a member /60. Literals only: never resolves a name. */
class Ipv6Prefix private constructor(private val bytes: ByteArray, val length: Int, private val text: String) {
    fun contains(address: String): Boolean {
        val other = parseAddress(address) ?: return false
        val full = length / 8
        for (i in 0 until full) if (bytes[i] != other[i]) return false
        val rest = length % 8
        if (rest == 0) return true
        val mask = (0xFF shl (8 - rest)) and 0xFF
        return (bytes[full].toInt() and mask) == (other[full].toInt() and mask)
    }

    override fun toString() = text

    /** The `::1` host of this network (a router's address, as hams-node gossips to `ADDRESS::1` of a peer's /60). */
    fun firstHost(): String {
        val host = bytes.copyOf()
        for (i in (length + 7) / 8 until 16) host[i] = 0
        if (length % 8 != 0) host[length / 8] = (host[length / 8].toInt() and (0xFF shl (8 - length % 8))).toByte()
        host[15] = 1
        return java.net.InetAddress.getByAddress(host).hostAddress
    }

    companion object {
        fun parse(text: String): Ipv6Prefix? {
            val parts = text.split("/")
            if (parts.size != 2) return null
            val length = parts[1].toIntOrNull() ?: return null
            if (length < 0 || length > 128) return null
            val raw = parseAddress(parts[0]) ?: return null
            return Ipv6Prefix(raw, length, text)
        }

        fun parseAddress(text: String): ByteArray? {
            if (!text.contains(':') || text.any { !(it.isLetterOrDigit() || it == ':' || it == '.') }) return null
            return try {
                (InetAddress.getByName(text) as? Inet6Address)?.address
            } catch (e: java.net.UnknownHostException) {
                null
            }
        }
    }
}
