package com.hams.net.core

/** Why a packet may not go to RF. */
enum class RfRefusal { ENCRYPTED, NOT_ALLOWED }

/**
 * What may leave on an RF interface: the readable-only allow list of the relay (`hamsnet_boundary::decide`, `hams_net_core::boundary`):
 * encrypted flows (ESP, WireGuard, IKE, OpenVPN, SSH, TLS ports) are refused; the first packet of a flow is allowed only toward
 * the readable services (tcp 80 and the integrity-only 8443, udp 53) or ICMPv6 echo; later packets of an allowed flow pass.
 * Only plain IPv6 packets with no extension headers are classified; anything else is refused as not allowed.
 */
class RfBoundary {
    private val encryptedUdp = setOf(51820, 5525, 500, 4500, 1194)
    private val encryptedTcp = setOf(22, 443, 853, 993, 995, 465, 8883, 1194)
    private val plainTcp = setOf(80, 8443)
    private val plainUdp = setOf(53)
    private val flows = mutableSetOf<String>()
    var refusedEncrypted = 0L; private set
    var refusedOther = 0L; private set

    /** null when the packet may go to RF. */
    @Synchronized
    fun check(packet: ByteArray): RfRefusal? {
        val verdict = decide(packet)
        when (verdict) {
            RfRefusal.ENCRYPTED -> refusedEncrypted++
            RfRefusal.NOT_ALLOWED -> refusedOther++
            null -> {}
        }
        return verdict
    }

    private fun port(packet: ByteArray, at: Int) = ((packet[at].toInt() and 0xFF) shl 8) or (packet[at + 1].toInt() and 0xFF)

    private fun decide(packet: ByteArray): RfRefusal? {
        if (!Ipv6Packet.isIpv6(packet)) return RfRefusal.NOT_ALLOWED
        val next = packet[6].toInt() and 0xFF
        val h = Ipv6Packet.HEADER
        if (next == 50) return RfRefusal.ENCRYPTED
        val key: String
        val allowedNew: Boolean
        when (next) {
            6, 17 -> {
                if (packet.size < h + 4) return RfRefusal.NOT_ALLOWED
                val dst = port(packet, h + 2)
                val tcp = next == 6
                if (dst in (if (tcp) encryptedTcp else encryptedUdp)) return RfRefusal.ENCRYPTED
                key = "$next/${Ipv6Packet.source(packet)}/${port(packet, h)}/${Ipv6Packet.destination(packet)}/$dst"
                allowedNew = dst in (if (tcp) plainTcp else plainUdp)
            }
            58 -> {
                if (packet.size < h + 1) return RfRefusal.NOT_ALLOWED
                return if ((packet[h].toInt() and 0xFF) == 128 || (packet[h].toInt() and 0xFF) == 129) null else RfRefusal.NOT_ALLOWED
            }
            else -> return RfRefusal.NOT_ALLOWED
        }
        if (key in flows) return null
        if (!allowedNew) return RfRefusal.NOT_ALLOWED
        flows += key
        return null
    }
}
