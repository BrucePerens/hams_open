package com.hams.net.core

/** What a link carries. Anything but NONE is encrypted or obscured, which 47 CFR 97.113(a)(4) forbids on RF. */
enum class LinkSuite { NONE, WIREGUARD, ENCRYPTED_OTHER }

/** The USB-OTG or Bluetooth serial line to the TNC or radio (a fake in tests). */
interface SerialPort {
    fun write(data: ByteArray)
}

/** Whether this app may key the radio now: the TX owner is this station and no demo lock is held. All keying goes through it. */
fun interface TxGate {
    fun mayKey(): Boolean
}

class RfRefused(message: String) : IllegalStateException(message)

object RfPolicy {
    fun requireUnencrypted(link: LinkType, suite: LinkSuite) {
        if (link == LinkType.RF && suite != LinkSuite.NONE) throw RfRefused("$suite is encrypted and is refused on an RF link (47 CFR 97.113(a)(4))")
    }

    fun stationIdReminder(callsign: String): String =
        "RF interface on. You are the control operator: identify with $callsign at least every 10 minutes and at the end of a communication (47 CFR 97.119). No encryption is used on RF."
}

/** An RF interface: IP packets go out unencrypted as KISS data frames, only when the [TxGate] allows keying. */
class RfInterface(private val port: SerialPort, private val callsign: String, private val gate: TxGate) {
    var enabled = false
        private set
    val link = LinkType.RF
    private val decoder = KissDecoder()

    /** Turns the interface on and returns the station-ID reminder to show. */
    fun enable(): String {
        require(callsign.isNotBlank()) { "an RF interface needs the operator's callsign" }
        enabled = true
        return RfPolicy.stationIdReminder(callsign)
    }

    fun disable() { enabled = false }

    /** Refuses WireGuard (or any encrypting suite) on this interface. */
    fun attach(suite: LinkSuite) = RfPolicy.requireUnencrypted(link, suite)

    /** Sends one packet; false (nothing written) unless enabled and the gate allows keying. */
    fun send(packet: ByteArray): Boolean {
        if (!enabled || !gate.mayKey()) return false
        port.write(Kiss.encode(packet))
        return true
    }

    /** Serial bytes in; the data-frame payloads (packets from the air). */
    fun receive(serial: ByteArray): List<ByteArray> = if (!enabled) emptyList() else decoder.feed(serial).filter { it.command == 0 }.map { it.payload }
}
