package com.hams.net.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class TxPolicyTest {
    private val packet = Pkt.readable

    private fun rf(demo: DemoLock, holds: TxHolds, tnc: VirtualTnc, blocklist: Blocklist = Blocklist()) =
        RfInterface(tnc, "N0CALL", OwnedTxGate(demo, holds, blocklist)).also { it.enable() }

    @Test fun withoutDemoModeEveryCheckPassesAndTheHoldIsClearedAfterwards() {
        val holds = TxHolds()
        val tnc = VirtualTnc()
        assertTrue(rf(DemoLock(false, holds), holds, tnc).send(packet))
        assertEquals(1, tnc.received.size)
        assertEquals(0, holds.count())
    }

    @Test fun demoLockRefusesKeyingUntilUnlockedWithPasswordAndCallsign() {
        val holds = TxHolds()
        val tnc = VirtualTnc()
        val demo = DemoLock(true, holds) { password, _ -> password == "right" }
        val iface = rf(demo, holds, tnc)
        assertFalse(iface.send(packet))
        assertFalse(demo.unlock("wrong", "N0CALL"))
        assertFalse(demo.unlock("right", " "))
        assertFalse(iface.send(packet))
        assertTrue(demo.unlock("right", "N0CALL"))
        assertTrue(iface.send(packet))
        demo.lock()
        assertFalse(iface.send(packet))
        assertEquals(1, tnc.received.size)
    }

    @Test fun lockingReleasesEveryHoldAndAReleasedHoldSendsNothing() {
        val holds = TxHolds()
        val demo = DemoLock(true, holds) { _, _ -> true }
        demo.unlock("x", "N0CALL")
        val claim = holds.claimPending("other")
        demo.lock()
        assertEquals(Settled.GONE, holds.confirm(claim))
        assertEquals(0, holds.count())
        // a lock that lands between claim and confirm: the gate sends nothing
        demo.unlock("x", "N0CALL")
        var sent = false
        val gate = OwnedTxGate(demo, holds, Blocklist())
        val ran = gate.key { sent = true }
        assertTrue(ran && sent)
        val racing = TxGate { send ->
            val c = holds.claimPending("racing")
            demo.lock()
            if (holds.confirm(c) == Settled.GONE) false else { send(); true }
        }
        sent = false
        assertFalse(racing.key { sent = true })
        assertFalse(sent)
    }

    @Test fun aStationSwitchedOffBySignedStatusDoesNotKey() {
        val holds = TxHolds()
        val tnc = VirtualTnc()
        val blocklist = Blocklist()
        val iface = rf(DemoLock(false, holds), holds, tnc, blocklist)
        blocklist.disabled = true
        assertFalse(iface.send(packet))
        assertEquals(0, tnc.received.size)
    }

    @Test fun theHoldIsClearedWhenTheKeyingWriteFails() {
        val holds = TxHolds()
        val gate = OwnedTxGate(DemoLock(false, holds), holds, Blocklist())
        assertFailsWith<IllegalStateException> { gate.key { error("serial gone") } }
        assertEquals(0, holds.count())
    }
}
