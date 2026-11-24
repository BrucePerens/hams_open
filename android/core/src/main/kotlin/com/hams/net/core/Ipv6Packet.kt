package com.hams.net.core

import java.net.InetAddress

/** The two addresses of a raw IPv6 packet from or for the VPN interface. */
object Ipv6Packet {
    const val HEADER = 40

    fun isIpv6(packet: ByteArray) = packet.size >= HEADER && (packet[0].toInt() and 0xF0) == 0x60

    fun source(packet: ByteArray): String? = address(packet, 8)

    fun destination(packet: ByteArray): String? = address(packet, 24)

    private fun address(packet: ByteArray, at: Int): String? =
        if (isIpv6(packet)) InetAddress.getByAddress(packet.copyOfRange(at, at + 16)).hostAddress else null
}
