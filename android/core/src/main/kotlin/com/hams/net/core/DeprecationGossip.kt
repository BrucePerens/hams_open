package com.hams.net.core

import uniffi.hams_net_ffi.DeprecationList
import uniffi.hams_net_ffi.ListChange
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException

const val GOSSIP_PORT = 5710
private const val TIMEOUT_MS = 5_000
private const val MAX_REPLY_BYTES = 262_144
private const val MAX_REQUEST_BYTES = 64

/**
 * Gossip of the signed deprecation list with a neighbouring router on tcp 5710 (docs/proposals/IPV6_DEPRECATION_LIST.md in hams_com):
 * send the line `HAMS-GOSSIP <held seq>`; a neighbour holding a newer list answers the chain of signed documents it got from hams.com,
 * which the Rust core verifies and orders by sequence number exactly like a document from hams.com. Nothing but those signed
 * documents is ever sent; bad or out of line documents are skipped. Never run on an RF link without the Part 97 review.
 */
object DeprecationGossip {
    /** Ask one neighbour; returns the changes of the documents that verified and lined up (empty on any failure). */
    fun ask(list: DeprecationList, host: String, port: Int = GOSSIP_PORT): List<ListChange> = try {
        Socket().use { socket ->
            socket.connect(InetSocketAddress(host, port), TIMEOUT_MS)
            socket.soTimeout = TIMEOUT_MS
            socket.getOutputStream().apply { write(list.gossipRequest().toByteArray()); flush() }
            val reply = socket.getInputStream().readNBytes(MAX_REPLY_BYTES)
            list.acceptGossip(String(reply, Charsets.UTF_8))
        }
    } catch (e: java.io.IOException) {
        emptyList()
    }
}

/** Answers neighbours on tcp 5710 (or `port`) with the stored signed chain when the held list is newer than theirs. */
class GossipServer(private val list: DeprecationList, port: Int = GOSSIP_PORT, bind: String? = null) {
    private val server = ServerSocket().apply {
        reuseAddress = true
        bind(if (bind == null) InetSocketAddress(port) else InetSocketAddress(bind, port))
    }
    private val worker = Thread(::serve, "hams-gossip").apply { isDaemon = true }

    val port: Int get() = server.localPort

    fun start() = worker.start()

    fun stop() {
        server.close()
    }

    private fun serve() {
        while (!server.isClosed) {
            try {
                server.accept().use(::answer)
            } catch (e: SocketException) {
                return // closed
            } catch (e: java.io.IOException) {
                continue
            }
        }
    }

    // One neighbour, one line each way; a slow or oversize request gets nothing.
    private fun answer(socket: Socket) {
        socket.soTimeout = TIMEOUT_MS
        val line = try {
            val bytes = java.io.ByteArrayOutputStream()
            val input = socket.getInputStream()
            while (bytes.size() < MAX_REQUEST_BYTES) {
                val b = input.read()
                if (b < 0 || b == '\n'.code) break
                bytes.write(b)
            }
            String(bytes.toByteArray(), Charsets.UTF_8)
        } catch (e: SocketTimeoutException) {
            return
        }
        val reply = list.gossipReply(line)
        if (reply.isNotEmpty()) socket.getOutputStream().apply { write(reply.toByteArray()); flush() }
    }
}
