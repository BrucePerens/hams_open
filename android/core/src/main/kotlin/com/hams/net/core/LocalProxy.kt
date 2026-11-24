package com.hams.net.core

import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket

/**
 * The app's HTTP proxy on loopback (also serves `/proxy.pac`): CONNECT and absolute-form requests for `.ham` names and ham-network
 * addresses go to the member's address through the tunnel; whitelisted sites go out directly; anything else gets 403. Listens on
 * 127.0.0.1 only.
 */
class LocalProxy(private val routing: HamRouting, private val resolver: HamResolver, private val whitelistDirect: Boolean = true) {
    private val server = ServerSocket(0, 16, InetAddress.getByName("127.0.0.1"))
    val port: Int get() = server.localPort
    private val acceptor = Thread(::accept, "hams-proxy").apply { isDaemon = true }

    fun start() = acceptor.start()
    fun stop() = server.close()

    private fun accept() {
        while (!server.isClosed) {
            val client = try { server.accept() } catch (e: java.io.IOException) { return }
            Thread({ client.use(::serve) }, "hams-proxy-conn").apply { isDaemon = true }.start()
        }
    }

    private fun serve(client: Socket) {
        client.soTimeout = 30_000
        try {
            val input = client.getInputStream()
            val head = readHead(input) ?: return
            val out = client.getOutputStream()
            val line = head.substringBefore("\r\n").split(" ")
            if (line.size < 3) return reply(out, 400)
            val method = line[0]
            val target = line[1]
            if (method == "GET" && target == "/proxy.pac") {
                val body = routing.pac("127.0.0.1:$port").toByteArray()
                out.write(("HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n").toByteArray() + body)
                return
            }
            val (host, port) = if (method == "CONNECT") hostPort(target, 443) else absoluteTarget(target) ?: return reply(out, 400)
            val upstream = open(host, port) ?: return reply(out, 403)
            upstream.use {
                if (method == "CONNECT") {
                    out.write("HTTP/1.1 200 Connection established\r\n\r\n".toByteArray())
                } else {
                    it.getOutputStream().write(head.toByteArray(Charsets.ISO_8859_1))
                }
                pipe(client, it)
            }
        } catch (e: java.io.IOException) {
            // connection ended
        }
    }

    /** The socket to the destination, or null when it is neither `.ham`, a ham-network address nor whitelisted. */
    private fun open(host: String, port: Int): Socket? {
        val address = when {
            Ipv6Prefix.parseAddress(host) != null -> if (routing.decideAddress(host) == Route.PROXY) host else return null
            routing.decideHost(host) == Route.PROXY ->
                if (host.lowercase().trimEnd('.').endsWith(".ham")) resolver.resolve(host) ?: return null
                else if (whitelistDirect) host else return null
            else -> return null
        }
        return try {
            Socket().also { it.connect(java.net.InetSocketAddress(address, port), 10_000) }
        } catch (e: java.io.IOException) {
            null
        }
    }

    private fun pipe(a: Socket, b: Socket) {
        val back = Thread({ copy(b.getInputStream(), a.getOutputStream()); runCatching { a.shutdownOutput() } }, "hams-proxy-pipe").apply { isDaemon = true }
        back.start()
        copy(a.getInputStream(), b.getOutputStream())
        runCatching { b.shutdownOutput() }
        back.join(60_000)
    }

    private fun copy(from: InputStream, to: OutputStream) {
        val buffer = ByteArray(16_384)
        try {
            while (true) {
                val n = from.read(buffer)
                if (n < 0) return
                to.write(buffer, 0, n)
                to.flush()
            }
        } catch (e: java.io.IOException) {
            // closed
        }
    }

    private fun readHead(input: InputStream): String? {
        val sb = StringBuilder()
        while (sb.length < 16_384) {
            val b = input.read()
            if (b < 0) return null
            sb.append(b.toChar())
            if (sb.endsWith("\r\n\r\n")) return sb.toString()
        }
        return null
    }

    private fun reply(out: OutputStream, code: Int) {
        out.write("HTTP/1.1 $code ${if (code == 403) "Forbidden" else "Bad Request"}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".toByteArray())
    }

    private fun hostPort(text: String, default: Int): Pair<String, Int> {
        if (text.startsWith("[")) {
            val end = text.indexOf(']')
            return text.substring(1, end) to (text.substring(end + 1).removePrefix(":").toIntOrNull() ?: default)
        }
        val at = text.lastIndexOf(':')
        return if (at > 0 && text.indexOf(':') == at) text.substring(0, at) to (text.substring(at + 1).toIntOrNull() ?: default) else text to default
    }

    private fun absoluteTarget(target: String): Pair<String, Int>? {
        if (!target.startsWith("http://")) return null
        return hostPort(target.removePrefix("http://").substringBefore('/'), 80)
    }
}
