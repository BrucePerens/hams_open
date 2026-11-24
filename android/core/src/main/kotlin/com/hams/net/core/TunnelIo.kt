package com.hams.net.core

import java.io.InputStream
import java.io.OutputStream
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetSocketAddress

/** One UDP socket for every peer. `protect` keeps it out of the VPN (VpnService.protect on a device; a no-op in tests). */
class UdpDatagrams(protect: (DatagramSocket) -> Unit = {}) : Datagrams {
    private val socket = DatagramSocket().also(protect)

    override fun send(to: InetSocketAddress, data: ByteArray) {
        try {
            socket.send(DatagramPacket(data, data.size, to))
        } catch (e: java.io.IOException) {
            // an unreachable endpoint loses the datagram; WireGuard retries
        }
    }

    /** Blocks, delivering datagrams to [engine], until [close]. */
    fun receiveLoop(engine: TunnelEngine) {
        val buffer = ByteArray(65_600)
        while (!socket.isClosed) {
            try {
                val p = DatagramPacket(buffer, buffer.size)
                socket.receive(p)
                engine.fromNetwork(InetSocketAddress(p.address, p.port), p.data.copyOfRange(p.offset, p.offset + p.length))
            } catch (e: java.io.IOException) {
                if (socket.isClosed) return
            }
        }
    }

    fun close() = socket.close()
}

/** Reads packets from the VPN interface into the engine and runs its timer; writes decrypted packets back. */
class TunLoop(private val input: InputStream, output: OutputStream, private val engine: TunnelEngine) {
    val sink = object : PacketSink {
        override fun write(packet: ByteArray) {
            synchronized(output) { output.write(packet) }
        }
    }

    @Volatile private var stopped = false
    private val timer = Thread({
        while (!stopped) {
            engine.tick()
            try { Thread.sleep(250) } catch (e: InterruptedException) { return@Thread }
        }
    }, "hams-wg-timer").apply { isDaemon = true }

    /** Blocks until the interface is closed. */
    fun run() {
        timer.start()
        val buffer = ByteArray(32_768)
        try {
            while (!stopped) {
                val n = input.read(buffer)
                if (n < 0) break
                if (n > 0) engine.fromTun(buffer.copyOf(n))
            }
        } catch (e: java.io.IOException) {
            // interface closed
        } finally {
            stopped = true
            timer.interrupt()
        }
    }

    fun stop() {
        stopped = true
        timer.interrupt()
    }
}
