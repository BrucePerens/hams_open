package com.hams.net.core

import java.io.InputStream

/** A TNC opened over some transport: the port to write KISS to, the stream to read it from, and how to close it. */
class OpenTnc(val port: SerialPort, val input: InputStream, val close: () -> Unit)

/** What the screen shows after asking to enable the RF interface. */
sealed class RfStart {
    /** Enabled; `reminder` is the station-ID reminder to keep on screen while it is on. */
    class On(val reminder: String) : RfStart()
    class Failed(val message: String) : RfStart()
}

/**
 * The RF section of the app screen: enabling needs a valid callsign and a TNC; enabling shows the station-ID reminder, disabling closes
 * the TNC. Keying goes through `gate` (an [OwnedTxGate] in the app). `onPacket` receives packets from the air.
 */
class RfController(private val gate: TxGate, private val onPacket: (ByteArray) -> Unit) {
    private var tnc: OpenTnc? = null
    var rf: RfInterface? = null
        private set

    @Synchronized
    fun enable(callsign: String, open: () -> OpenTnc?): RfStart {
        val call = callsign.trim().uppercase()
        if (!CALLSIGN.matches(call)) return RfStart.Failed("Enter your amateur callsign to use RF.")
        if (rf != null) return RfStart.Failed("The RF interface is already on.")
        val opened = try { open() } catch (e: java.io.IOException) { null } ?: return RfStart.Failed("No TNC found. Connect a USB TNC or pair a Bluetooth TNC.")
        val iface = RfInterface(opened.port, call, gate)
        val reminder = iface.enable()
        tnc = opened
        rf = iface
        Thread { SerialPump(opened.input, iface, onPacket).run() }.apply { isDaemon = true }.start()
        return RfStart.On(reminder)
    }

    @Synchronized
    fun disable() {
        rf?.disable()
        tnc?.close?.invoke()
        rf = null
        tnc = null
    }

    private companion object {
        val CALLSIGN = Regex("[A-Z0-9]{1,3}[0-9][A-Z0-9]{0,3}[A-Z]")
    }
}
