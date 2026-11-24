package com.hams.net.core

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RfControllerTest {
    private val open = TxGate { send -> send(); true }
    private fun tnc(closed: BooleanArray = BooleanArray(1)) = OpenTnc(StreamSerialPort(ByteArrayOutputStream()), ByteArrayInputStream(ByteArray(0))) { closed[0] = true }

    @Test fun enablingShowsTheReminderForTheCallsign() {
        val c = RfController(open) {}
        val r = c.enable(" n0call ", { tnc() })
        assertTrue(r is RfStart.On && r.reminder.contains("N0CALL") && r.reminder.contains("97.119"))
        assertTrue(c.rf!!.enabled)
    }

    @Test fun noCallsignOrNoTncMeansNothingIsEnabled() {
        val c = RfController(open) {}
        assertTrue(c.enable("", { tnc() }) is RfStart.Failed)
        assertTrue(c.enable("not a call", { tnc() }) is RfStart.Failed)
        assertTrue(c.enable("N0CALL", { null }) is RfStart.Failed)
        assertEquals(null, c.rf)
    }

    @Test fun disablingClosesTheTncAndStopsKeying() {
        val closed = BooleanArray(1)
        val c = RfController(open) {}
        c.enable("N0CALL", { tnc(closed) })
        val rf = c.rf!!
        c.disable()
        assertTrue(closed[0])
        assertFalse(rf.send(Pkt.readable))
        assertEquals(null, c.rf)
    }
}
