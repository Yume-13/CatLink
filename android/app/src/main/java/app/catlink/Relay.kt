/*
 * MIT License
 *
 * Copyright © 2026 Yume-13
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

package app.catlink

import android.content.Context
import android.net.ConnectivityManager
import android.net.DnsResolver
import android.net.LocalServerSocket
import android.net.LocalSocket
import android.net.LocalSocketAddress
import android.net.Network
import android.os.Build
import android.system.Os
import android.system.OsConstants
import android.system.StructTimeval
import android.util.Log
import java.io.BufferedInputStream
import java.io.DataInputStream
import java.io.EOFException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.ConnectException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NoRouteToHostException
import java.net.Socket
import java.net.SocketTimeoutException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

/**
 * Accepts streams from the PC (via `adb forward tcp:N localabstract:catlink`) and
 * opens the real connections from this app. See windows/src/proto.rs for the format.
 */
class Relay(private val context: Context) {
    companion object {
        const val TAG = "CatLink"
        const val SOCKET_NAME = "catlink"
        private val MAGIC = byteArrayOf('C'.code.toByte(), 'T'.code.toByte(), 'L'.code.toByte(), 'K'.code.toByte())
        private const val VERSION = 1

        private const val KIND_HELLO = 0
        private const val KIND_TCP = 1
        private const val KIND_UDP = 2
        private const val KIND_DNS = 3
        private const val KIND_ICMP = 4

        private const val ST_OK = 0
        private const val ST_FAILED = 1
        private const val ST_REFUSED = 2
        private const val ST_UNREACHABLE = 3
        private const val ST_TIMEOUT = 4
        private const val ST_BAD_REQUEST = 5
        private const val ST_BAD_VERSION = 6

        private const val CONNECT_TIMEOUT_MS = 15_000
        private const val UDP_IDLE_MS = 120_000

        /** adbd runs as shell (2000), or root (0) after `adb root`. */
        private val ALLOWED_UIDS = setOf(0, 2000)
    }

    private val pool: ExecutorService = Executors.newCachedThreadPool { r ->
        Thread(r, "catlink-io").apply { isDaemon = true }
    }
    private var server: LocalServerSocket? = null
    @Volatile private var running = false

    fun start() {
        if (running) return
        server = LocalServerSocket(SOCKET_NAME)
        running = true
        Thread({ acceptLoop() }, "catlink-accept").start()
        Log.i(TAG, "listening on @$SOCKET_NAME")
    }

    fun stop() {
        if (!running) return
        running = false
        // LocalServerSocket.close() does not wake a blocked accept(); poke it.
        try {
            LocalSocket().use { it.connect(LocalSocketAddress(SOCKET_NAME)) }
        } catch (_: IOException) {
        }
        try {
            server?.close()
        } catch (_: IOException) {
        }
        server = null
        pool.shutdownNow()
    }

    private fun acceptLoop() {
        val srv = server ?: return
        while (running) {
            val client = try {
                srv.accept()
            } catch (e: IOException) {
                if (running) Log.w(TAG, "accept failed", e)
                break
            }
            if (!running) {
                client.close(); break
            }
            val uid = try { client.peerCredentials.uid } catch (_: IOException) { -1 }
            if (uid !in ALLOWED_UIDS) {
                Log.w(TAG, "rejected connection from uid $uid")
                client.close()
                continue
            }
            try {
                pool.execute { handle(client) }
            } catch (_: Exception) {
                client.close()
            }
        }
    }

    private fun handle(client: LocalSocket) {
        RelayStats.markPeer()
        try {
            val input = DataInputStream(BufferedInputStream(client.inputStream, 64 * 1024))
            val output = client.outputStream
            val magic = ByteArray(4)
            input.readFully(magic)
            if (!magic.contentEquals(MAGIC)) {
                output.write(ST_BAD_REQUEST); return
            }
            if (input.readUnsignedByte() != VERSION) {
                output.write(ST_BAD_VERSION); return
            }
            val kind = input.readUnsignedByte()
            val addr = when (input.readUnsignedByte()) {
                4 -> ByteArray(4).also { input.readFully(it) }
                6 -> ByteArray(16).also { input.readFully(it) }
                else -> { output.write(ST_BAD_REQUEST); return }
            }
            val port = input.readUnsignedShort()
            val ip = InetAddress.getByAddress(addr)
            when (kind) {
                KIND_HELLO -> {
                    // The PC uses the port field for flags: bit 0 = connected over Wi-Fi (wireless debugging or adb tcpip).
                    Uplink.onHello(port and 1 != 0)
                    Uplink.update(context, RelayService.running)
                    output.write(ST_OK)
                    writeFrame(output, describe().toByteArray())
                }
                KIND_TCP -> counted { relayTcp(input, output, client, InetSocketAddress(ip, port)) }
                KIND_UDP -> counted { relayUdp(input, output, InetSocketAddress(ip, port)) }
                KIND_DNS -> counted { relayDns(input, output) }
                KIND_ICMP -> relayPing(input, output, ip)
                else -> output.write(ST_BAD_REQUEST)
            }
        } catch (_: EOFException) {
        } catch (e: IOException) {
            Log.d(TAG, "stream ended: $e")
        } finally {
            try { client.close() } catch (_: IOException) {}
        }
    }

    private inline fun counted(block: () -> Unit) {
        RelayStats.active.incrementAndGet()
        RelayStats.total.incrementAndGet()
        try { block() } finally { RelayStats.active.decrementAndGet() }
    }

    /** key=value lines; the PC shows these in its tray flyout. */
    private fun describe(): String {
        val net = Uplink.effective(context)
        val maker = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }
        val model = if (Build.MODEL.startsWith(maker, ignoreCase = true)) Build.MODEL else "$maker ${Build.MODEL}"
        return listOf(
            "version=${BuildConfigCompat.versionName(context)}",
            "model=$model",
            "android=${Build.VERSION.RELEASE}",
            "network=${net.kind.label}",
            "carrier=${net.carrier}",
        ).joinToString("\n")
    }

    private fun statusFor(e: IOException): Int = when (e) {
        is ConnectException -> if (e.message?.contains("ECONNREFUSED") == true) ST_REFUSED else ST_UNREACHABLE
        is SocketTimeoutException -> ST_TIMEOUT
        is NoRouteToHostException -> ST_UNREACHABLE
        else -> ST_FAILED
    }

    // ---- TCP ---------------------------------------------------------------

    private fun relayTcp(input: InputStream, output: OutputStream, client: LocalSocket, dst: InetSocketAddress) {
        val remote = Socket()
        try {
            try {
                Uplink.network()?.bindSocket(remote)
                remote.tcpNoDelay = true
                remote.connect(dst, CONNECT_TIMEOUT_MS)
            } catch (e: IOException) {
                output.write(statusFor(e))
                return
            }
            output.write(ST_OK)
            output.flush()
            val remoteIn = remote.getInputStream()
            val remoteOut = remote.getOutputStream()
            // PC -> internet on a pool thread, internet -> PC on this one.
            val upload = pool.submit {
                try {
                    pump(input, remoteOut, RelayStats.bytesUp)
                    remote.shutdownOutput()
                } catch (_: IOException) {
                    try { remote.close() } catch (_: IOException) {}
                }
            }
            try {
                pump(remoteIn, output, RelayStats.bytesDown)
                client.shutdownOutput()
            } catch (_: IOException) {
            }
            try { upload.get() } catch (_: Exception) {}
        } finally {
            try { remote.close() } catch (_: IOException) {}
        }
    }

    private fun pump(from: InputStream, to: OutputStream, counter: AtomicLong) {
        val buf = ByteArray(64 * 1024)
        while (true) {
            val n = from.read(buf)
            if (n < 0) return
            to.write(buf, 0, n)
            counter.addAndGet(n.toLong())
        }
    }

    // ---- framing -----------------------------------------------------------

    private fun writeFrame(out: OutputStream, data: ByteArray, len: Int = data.size) {
        val frame = ByteArray(2 + len)
        frame[0] = (len ushr 8).toByte()
        frame[1] = len.toByte()
        System.arraycopy(data, 0, frame, 2, len)
        synchronized(out) { out.write(frame); out.flush() }
    }

    /** Returns null on clean EOF. */
    private fun readFrame(input: DataInputStream): ByteArray? {
        val len = try { input.readUnsignedShort() } catch (_: EOFException) { return null }
        return ByteArray(len).also { input.readFully(it) }
    }

    // ---- UDP ---------------------------------------------------------------

    private fun relayUdp(input: DataInputStream, output: OutputStream, dst: InetSocketAddress) {
        val sock = try {
            DatagramSocket().apply {
                Uplink.network()?.bindSocket(this)
                connect(dst)
                soTimeout = UDP_IDLE_MS
            }
        } catch (_: IOException) {
            output.write(ST_UNREACHABLE); return
        }
        sock.use {
            output.write(ST_OK)
            output.flush()
            val upload = pool.submit {
                try {
                    while (true) {
                        val d = readFrame(input) ?: break
                        sock.send(DatagramPacket(d, d.size))
                        RelayStats.bytesUp.addAndGet(d.size.toLong())
                    }
                } catch (_: IOException) {
                }
                sock.close()
            }
            val buf = ByteArray(65535)
            try {
                while (!sock.isClosed) {
                    val p = DatagramPacket(buf, buf.size)
                    sock.receive(p)
                    writeFrame(output, buf, p.length)
                    RelayStats.bytesDown.addAndGet(p.length.toLong())
                }
            } catch (_: IOException) {
                // idle timeout, or the PC side closed
            }
            upload.cancel(true)
        }
    }

    // ---- DNS ---------------------------------------------------------------

    /** Queries go through Android's own resolver (respects Private DNS) on API 29+. */
    private fun relayDns(input: DataInputStream, output: OutputStream) {
        output.write(ST_OK)
        output.flush()
        while (true) {
            val query = readFrame(input) ?: return
            if (query.size < 12) continue
            RelayStats.bytesUp.addAndGet(query.size.toLong())
            resolve(query) { answer ->
                // Keep the transaction id the PC used.
                answer[0] = query[0]; answer[1] = query[1]
                try {
                    writeFrame(output, answer)
                    RelayStats.bytesDown.addAndGet(answer.size.toLong())
                } catch (_: IOException) {
                }
            }
        }
    }

    private fun resolve(query: ByteArray, done: (ByteArray) -> Unit) {
        val net = Uplink.network()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            DnsResolver.getInstance().rawQuery(
                net, query, DnsResolver.FLAG_EMPTY, pool, null,
                object : DnsResolver.Callback<ByteArray> {
                    override fun onAnswer(answer: ByteArray, rcode: Int) = done(answer)
                    override fun onError(error: DnsResolver.DnsException) {
                        Log.d(TAG, "rawQuery failed: $error; falling back to plain DNS")
                        plainDns(query, net)?.let(done)
                    }
                })
        } else {
            pool.execute { plainDns(query, net)?.let(done) }
        }
    }

    private fun plainDns(query: ByteArray, net: Network?): ByteArray? {
        val cm = context.getSystemService(ConnectivityManager::class.java)
        val servers = (net ?: cm.activeNetwork)?.let { cm.getLinkProperties(it)?.dnsServers }.orEmpty()
        val targets = servers.ifEmpty { listOf(InetAddress.getByName("8.8.8.8")) }
        for (server in targets) {
            try {
                DatagramSocket().use { s ->
                    net?.bindSocket(s)
                    s.soTimeout = 4000
                    s.send(DatagramPacket(query, query.size, server, 53))
                    val buf = ByteArray(4096)
                    val p = DatagramPacket(buf, buf.size)
                    s.receive(p)
                    return buf.copyOf(p.length)
                }
            } catch (_: IOException) {
            }
        }
        return null
    }

    // ---- ICMP echo -----------------------------------------------------------

    /** Real ping from the phone using an unprivileged ICMP socket (IPv4). */
    private fun relayPing(input: DataInputStream, output: OutputStream, ip: InetAddress) {
        if (ip !is Inet4Address) {
            output.write(ST_BAD_REQUEST); return
        }
        val fd = try {
            Os.socket(OsConstants.AF_INET, OsConstants.SOCK_DGRAM, OsConstants.IPPROTO_ICMP)
        } catch (_: Exception) {
            output.write(ST_FAILED); return
        }
        try {
            Uplink.network()?.bindSocket(fd)
            output.write(ST_OK)
            output.flush()
            val data = readFrame(input) ?: return
            Os.setsockoptTimeval(fd, OsConstants.SOL_SOCKET, OsConstants.SO_RCVTIMEO, StructTimeval.fromMillis(4000))
            val packet = ByteArray(8 + data.size)
            packet[0] = 8 // echo request; the kernel fills in id and checksum, seq stays 0
            System.arraycopy(data, 0, packet, 8, data.size)
            Os.sendto(fd, packet, 0, packet.size, 0, ip, 0)
            val reply = ByteArray(8 + data.size + 64)
            val n = Os.recvfrom(fd, reply, 0, reply.size, 0, null)
            if (n >= 8 && reply[0].toInt() == 0) {
                writeFrame(output, reply, n)
            }
        } catch (_: Exception) {
            // timeout or unreachable: close without a reply
        } finally {
            try { Os.close(fd) } catch (_: Exception) {}
        }
    }
}

object BuildConfigCompat {
    fun versionName(context: Context): String = try {
        context.packageManager.getPackageInfo(context.packageName, 0).versionName ?: "?"
    } catch (_: Exception) {
        "?"
    }
}
