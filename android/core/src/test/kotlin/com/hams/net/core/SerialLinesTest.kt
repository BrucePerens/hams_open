package com.hams.net.core

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class FakeUsbLine(private val incoming: List<ByteArray?>) : UsbLine {
    val writes = mutableListOf<ByteArray>()
    private var next = 0
    override fun write(data: ByteArray, timeoutMs: Int) { writes += data }
    override fun read(buffer: ByteArray, timeoutMs: Int): Int {
        if (next >= incoming.size) return -1
        val chunk = incoming[next++] ?: return 0
        chunk.copyInto(buffer)
        return chunk.size
    }
}

class SerialLinesTest {
    private val open = TxGate { send -> send(); true }

    @Test fun bluetoothStreamCarriesKissBothWays() {
        val out = ByteArrayOutputStream()
        val rf = RfInterface(StreamSerialPort(out), "N0CALL", open).also { it.enable() }
        val packet = Pkt.readable
        assertTrue(rf.send(packet))
        assertContentEquals(Kiss.encode(packet), out.toByteArray())
        val got = mutableListOf<ByteArray>()
        val air = Kiss.encode(packet)
        SerialPump(ByteArrayInputStream(air + air), rf) { got += it }.run()
        assertEquals(2, got.size)
        assertContentEquals(packet, got[0])
    }

    @Test fun usbWritesInChunksAndReadsThroughTimeoutsUntilClosed() {
        val packet = Pkt.readable
        val air = Kiss.encode(packet)
        val line = FakeUsbLine(listOf(air.copyOfRange(0, 5), null, air.copyOfRange(5, air.size)))
        val port = UsbSerialPort(line, chunk = 16)
        val rf = RfInterface(port, "N0CALL", open).also { it.enable() }
        assertTrue(rf.send(packet))
        assertTrue(line.writes.size > 1 && line.writes.all { it.size <= 16 })
        assertContentEquals(Kiss.encode(packet), line.writes.fold(ByteArray(0)) { a, b -> a + b })
        val got = mutableListOf<ByteArray>()
        SerialPump(port.input(), rf) { got += it }.run()
        assertEquals(1, got.size)
        assertContentEquals(packet, got[0])
    }

    @Test fun anInterfaceThatIsNotEnabledDeliversNothing() {
        val rf = RfInterface(StreamSerialPort(ByteArrayOutputStream()), "N0CALL", open)
        val got = mutableListOf<ByteArray>()
        SerialPump(ByteArrayInputStream(Kiss.encode(Pkt.readable)), rf) { got += it }.run()
        assertEquals(0, got.size)
    }
}
