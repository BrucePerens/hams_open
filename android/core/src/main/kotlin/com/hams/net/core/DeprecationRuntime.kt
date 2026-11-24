package com.hams.net.core

import uniffi.hams_net_ffi.DeprecationList
import uniffi.hams_net_ffi.ListChange
import uniffi.hams_net_ffi.PlatformCrypto
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.TimeUnit

const val POLL_SECONDS = 120L

/**
 * The deprecation channel of the app: a status poll every two minutes with the device token, gossip with the neighbouring routers
 * (the on-demand peers' `::1`, plus a listener for their requests), and enforcement of whatever verifies. [gossipBind] is the
 * tunnel address the listener binds to, so it is never open on the mobile network. `post(path, jsonBody)` posts to hams.com.
 */
class DeprecationRuntime(
    enrolment: Enrolment,
    crypto: PlatformCrypto,
    post: (String, String) -> String?,
    private val table: PeerTable,
    val blocklist: Blocklist,
    private val gossipBind: String?,
    private val gossipPort: Int = GOSSIP_PORT,
    private val neighbourPort: Int = GOSSIP_PORT,
    private val extraNeighbours: () -> List<String> = { emptyList() },
) {
    val list: DeprecationList = DeprecationList(crypto, enrolment.deprecationKey)
    private val enforcer = DeprecationEnforcer(table, blocklist)
    private val poller = DeprecationPoller(post, enrolment.token, enrolment.deprecationKey, crypto, list, object : DeprecationSink {
        override fun onChange(change: ListChange) = enforcer.apply(change)
        override fun onDisabled(reason: String) = enforcer.disable()
        override fun onEnabled() = enforcer.enable()
    })
    private var server: GossipServer? = null
    private var scheduler: ScheduledExecutorService? = null

    /** One status poll, then one gossip round with every neighbour. Never throws. */
    fun runOnce() {
        try {
            poller.poll()
            gossipOnce()
        } catch (e: RuntimeException) {
            // no information: nothing changes
        }
    }

    fun gossipOnce() {
        if (blocklist.disabled) return
        val hosts = (table.peerNetworks().map { it.firstHost() } + extraNeighbours()).distinct()
        for (host in hosts) for (change in DeprecationGossip.ask(list, host, neighbourPort)) enforcer.apply(change)
    }

    fun start() {
        if (gossipBind != null) server = GossipServer(list, gossipPort, gossipBind).also { it.start() }
        scheduler = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "hams-deprecation").apply { isDaemon = true } }
            .also { it.scheduleWithFixedDelay(::runOnce, 0, POLL_SECONDS, TimeUnit.SECONDS) }
    }

    fun stop() {
        scheduler?.shutdownNow()
        server?.stop()
    }
}
