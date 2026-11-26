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
import android.net.NetworkCapabilities
import android.telephony.TelephonyManager
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong

/** Throughput for one second, in bytes/s. */
data class Rate(val down: Float, val up: Float)

/** Live counters shared by the relay, the notification and the UI. */
object RelayStats {
    const val HISTORY = 60
    private const val PEER_TIMEOUT_MS = 8_000L

    val active = AtomicLong()
    val total = AtomicLong()
    val bytesUp = AtomicLong()   // PC -> internet
    val bytesDown = AtomicLong() // internet -> PC

    @Volatile var lastPeerSeen = 0L
        private set
    @Volatile var peerSince = 0L
        private set

    /** Oldest first, one entry per second while the service runs. */
    @Volatile var history: List<Rate> = emptyList()
        private set

    private var lastUp = 0L
    private var lastDown = 0L
    private var lastSampleAt = 0L

    fun markPeer() {
        val now = System.currentTimeMillis()
        if (now - lastPeerSeen > PEER_TIMEOUT_MS) peerSince = now
        lastPeerSeen = now
    }

    fun pcConnected(): Boolean = System.currentTimeMillis() - lastPeerSeen < PEER_TIMEOUT_MS

    /** Called once a second by the service. */
    @Synchronized
    fun sample() {
        val now = System.currentTimeMillis()
        val up = bytesUp.get()
        val down = bytesDown.get()
        if (lastSampleAt != 0L) {
            val dt = ((now - lastSampleAt).coerceAtLeast(200)) / 1000f
            val r = Rate((down - lastDown) / dt, (up - lastUp) / dt)
            history = (history + r).takeLast(HISTORY)
        }
        lastUp = up; lastDown = down; lastSampleAt = now
    }

    @Synchronized
    fun reset() {
        active.set(0); total.set(0); bytesUp.set(0); bytesDown.set(0)
        lastPeerSeen = 0; peerSince = 0
        history = emptyList()
        lastUp = 0; lastDown = 0; lastSampleAt = 0
    }
}

/** What the phone is currently using to reach the internet. */
data class NetInfo(val kind: Kind, val carrier: String) {
    enum class Kind(val label: String) { Mobile("Mobile data"), WiFi("Wi-Fi"), Ethernet("Ethernet"), Vpn("VPN"), Other("Network"), None("No network") }

    companion object {
        fun current(context: Context): NetInfo {
            val cm = context.getSystemService(ConnectivityManager::class.java)
            val caps = cm.activeNetwork?.let { cm.getNetworkCapabilities(it) }
            val kind = when {
                caps == null -> Kind.None
                caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> Kind.Vpn
                caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> Kind.Mobile
                caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> Kind.WiFi
                caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> Kind.Ethernet
                else -> Kind.Other
            }
            val carrier = if (kind == Kind.Mobile) {
                context.getSystemService(TelephonyManager::class.java)?.networkOperatorName.orEmpty()
            } else ""
            return NetInfo(kind, carrier)
        }
    }
}

object Fmt {
    fun bytes(b: Long): String {
        val units = arrayOf("B", "KB", "MB", "GB", "TB")
        var v = b.toDouble()
        var i = 0
        while (v >= 1024 && i < units.size - 1) { v /= 1024; i++ }
        return when {
            i == 0 -> "$b B"
            v >= 100 -> String.format(Locale.US, "%.0f %s", v, units[i])
            else -> String.format(Locale.US, "%.1f %s", v, units[i])
        }
    }

    fun rate(bps: Float): String = bytes(bps.toLong().coerceAtLeast(0)) + "/s"

    fun duration(ms: Long): String {
        val s = ms / 1000
        return when {
            s < 60 -> "$s s"
            s < 3600 -> "${s / 60} min"
            else -> "${s / 3600} h ${(s % 3600) / 60} min"
        }
    }
}
