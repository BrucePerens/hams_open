package com.hams.net.core

import java.io.IOException
import java.io.InputStream
import java.io.OutputStream

/** A [SerialPort] over a byte stream: the Bluetooth SPP (RFCOMM) socket's output stream, or any other. */
class StreamSerialPort(private val out: OutputStream) : SerialPort {
    override fun write(data: ByteArray) {
        out.write(data)
        out.flush()
    }
}

/** What the USB serial library's port offers (usb-serial-for-android's `UsbSerialPort`); a fake in tests. */
interface UsbLine {
    /** Writes all of `data` or throws. */
    fun write(data: ByteArray, timeoutMs: Int)

    /** Bytes read into `buffer`, 0 on timeout, -1 when the line is closed. */
    fun read(buffer: ByteArray, timeoutMs: Int): Int
}

/** A [SerialPort] over a USB-OTG TNC; frames are written in chunks the endpoint accepts. */
class UsbSerialPort(private val line: UsbLine, private val chunk: Int = 64, private val timeoutMs: Int = 1000) : SerialPort {
    override fun write(data: ByteArray) {
        var at = 0
        while (at < data.size) {
            val n = minOf(chunk, data.size - at)
            line.write(data.copyOfRange(at, at + n), timeoutMs)
            at += n
        }
    }

    /** The receive side as a stream, for [SerialPump]. */
    fun input(): InputStream = object : InputStream() {
        override fun read(): Int {
            val one = ByteArray(1)
            return if (read(one, 0, 1) < 0) -1 else one[0].toInt() and 0xFF
        }

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            val buffer = ByteArray(len)
            while (true) {
                val n = line.read(buffer, timeoutMs)
                if (n < 0) return -1
                if (n > 0) {
                    buffer.copyInto(b, off, 0, n)
                    return n
                }
            }
        }
    }
}

/** Reads the TNC's serial bytes until the line closes and hands each received packet to `onPacket`. Blocking: run it on its own thread. */
class SerialPump(private val input: InputStream, private val rf: RfInterface, private val onPacket: (ByteArray) -> Unit) {
    fun run() {
        val buffer = ByteArray(512)
        while (true) {
            val n = try { input.read(buffer) } catch (e: IOException) { -1 }
            if (n < 0) return
            for (packet in rf.receive(buffer.copyOf(n))) onPacket(packet)
        }
    }
}
