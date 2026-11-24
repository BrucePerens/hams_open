package com.hams.net.core

import java.io.ByteArrayOutputStream

/** KISS framing for a serial TNC (FEND 0xC0, FESC 0xDB). Command byte: port in the high nibble, 0 = data frame. */
object Kiss {
    const val FEND = 0xC0
    const val FESC = 0xDB
    const val TFEND = 0xDC
    const val TFESC = 0xDD

    fun encode(payload: ByteArray, port: Int = 0, command: Int = 0): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(FEND)
        out.write(((port and 0x0F) shl 4) or (command and 0x0F))
        for (b in payload) {
            when (b.toInt() and 0xFF) {
                FEND -> { out.write(FESC); out.write(TFEND) }
                FESC -> { out.write(FESC); out.write(TFESC) }
                else -> out.write(b.toInt())
            }
        }
        out.write(FEND)
        return out.toByteArray()
    }
}

class KissFrame(val port: Int, val command: Int, val payload: ByteArray)

/** Rebuilds frames from serial bytes in any chunking; a bad escape drops that frame. */
class KissDecoder {
    private val buf = ByteArrayOutputStream()
    private var escaped = false
    private var bad = false

    fun feed(data: ByteArray): List<KissFrame> {
        val frames = mutableListOf<KissFrame>()
        for (raw in data) {
            val b = raw.toInt() and 0xFF
            if (b == Kiss.FEND) {
                val bytes = buf.toByteArray()
                if (!bad && bytes.isNotEmpty()) {
                    val head = bytes[0].toInt() and 0xFF
                    frames += KissFrame(head shr 4, head and 0x0F, bytes.copyOfRange(1, bytes.size))
                }
                buf.reset(); escaped = false; bad = false
            } else if (escaped) {
                escaped = false
                when (b) {
                    Kiss.TFEND -> buf.write(Kiss.FEND)
                    Kiss.TFESC -> buf.write(Kiss.FESC)
                    else -> bad = true
                }
            } else if (b == Kiss.FESC) {
                escaped = true
            } else {
                buf.write(b)
            }
        }
        return frames
    }
}
