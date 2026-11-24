package com.hams.net.core

/** The result of confirming a pending hold: still ours, or released (by the demo lock) while we were getting ready to key. */
enum class Settled { HELD, GONE }

/**
 * Who keyed the transmitter (a port of the relay's `tx_ownership::TxHolds`, daemons/hams_local_relay): every transmission is a hold
 * claimed before keying, confirmed just before the keying write (a hold released meanwhile is [Settled.GONE] and nothing is sent),
 * and cleared when it is over. [releaseAll] ends every hold at once; it is how the demo lock takes the transmitter away.
 */
class TxHolds {
    class Claim(val what: String) { @Volatile internal var released = false }

    private val holds = mutableSetOf<Claim>()

    @Synchronized fun claimPending(what: String): Claim = Claim(what).also { holds += it }

    @Synchronized fun confirm(claim: Claim): Settled = if (claim.released || claim !in holds) Settled.GONE else Settled.HELD

    @Synchronized fun clear(claim: Claim) { holds -= claim }

    @Synchronized fun releaseAll(): Int {
        val n = holds.size
        for (c in holds) c.released = true
        holds.clear()
        return n
    }

    @Synchronized fun count() = holds.size
}

/**
 * The relay's demo lock (`demo_lock.rs`): [enforcing] is false unless the app is built or started as a locked demo, and then every check
 * passes. When enforcing the station starts locked and may only listen; [lock] releases every hold first, then locks; [unlock] needs the
 * password and an operator callsign (the relay's `demo_password.rs` rules; the check is injected so the same rules apply).
 */
class DemoLock(val enforcing: Boolean, private val holds: TxHolds, private val passwordOk: (String, String) -> Boolean = { _, _ -> false }) {
    @Volatile private var locked = enforcing

    val isLocked: Boolean get() = enforcing && locked

    fun lock() {
        if (!enforcing) return
        holds.releaseAll()
        locked = true
    }

    fun unlock(password: String, callsign: String): Boolean {
        if (!enforcing || callsign.isBlank() || !passwordOk(password, callsign)) return false
        locked = false
        return true
    }
}

/**
 * The [TxGate] with the relay's rules for keying (`native_packet_tnc::keying_hooks`): refused while the demo lock is on, refused while a
 * signed status has switched this station off ([Blocklist.disabled]), and otherwise a hold is claimed, confirmed (released meanwhile
 * means nothing is sent) and cleared afterwards, even if the write throws. Unkeying is never refused (this gate does not touch it).
 */
class OwnedTxGate(private val demo: DemoLock, private val holds: TxHolds, private val blocklist: Blocklist, private val what: String = "android rf interface") : TxGate {
    override fun key(send: () -> Unit): Boolean {
        if (demo.isLocked || blocklist.disabled) return false
        val claim = holds.claimPending(what)
        try {
            if (holds.confirm(claim) == Settled.GONE) return false
            send()
            return true
        } finally {
            holds.clear(claim)
        }
    }
}
