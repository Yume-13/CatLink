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
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.telephony.TelephonyManager
import android.util.Log

/**
 * Selects the network that carries the PC's relayed traffic.
 *
 * By default this is Android's default network. While the PC is connected over wireless
 * debugging and the "Use mobile data over Wi-Fi" setting is on, mobile data is requested
 * and every relayed socket is bound to it, falling back to the default network when
 * mobile data is unavailable.
 */
object Uplink {
    private const val PREFS = "catlink"
    private const val KEY_MOBILE_OVER_WIFI = "mobile_over_wifi"
    /** The PC sends a hello every 2 s; after this long without one, the link type is reset. */
    private const val LINK_TIMEOUT_MS = 8_000L

    @Volatile private var wirelessSeenAt = 0L
    @Volatile private var cellular: Network? = null
    private var callback: ConnectivityManager.NetworkCallback? = null

    /** Called for every hello; `wireless` is the flag the PC sends (see proto.rs). */
    fun onHello(wireless: Boolean) {
        wirelessSeenAt = if (wireless) System.currentTimeMillis() else 0L
    }

    /** Whether the PC is currently connected over Wi-Fi. */
    fun wirelessLink(): Boolean = System.currentTimeMillis() - wirelessSeenAt < LINK_TIMEOUT_MS

    fun mobileOverWifi(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_MOBILE_OVER_WIFI, true)

    fun setMobileOverWifi(context: Context, on: Boolean) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_MOBILE_OVER_WIFI, on).apply()
        update(context, RelayService.running)
    }

    private fun wanted(context: Context, running: Boolean) = running && wirelessLink() && mobileOverWifi(context)

    /**
     * Requests mobile data while it is wanted and releases it otherwise. Called once a
     * second by the service and whenever the link type or setting changes.
     */
    @Synchronized
    fun update(context: Context, running: Boolean) {
        val cm = context.getSystemService(ConnectivityManager::class.java)
        if (wanted(context, running)) {
            if (callback == null) {
                val cb = object : ConnectivityManager.NetworkCallback() {
                    override fun onAvailable(network: Network) {
                        Log.i(Relay.TAG, "mobile data up for the relay: $network")
                        cellular = network
                    }

                    override fun onLost(network: Network) {
                        if (cellular == network) cellular = null
                    }
                }
                val req = NetworkRequest.Builder()
                    .addTransportType(NetworkCapabilities.TRANSPORT_CELLULAR)
                    .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                    .build()
                try {
                    cm.requestNetwork(req, cb)
                    callback = cb
                } catch (e: Exception) {
                    Log.w(Relay.TAG, "could not request mobile data", e)
                }
            }
        } else {
            callback?.let {
                try { cm.unregisterNetworkCallback(it) } catch (_: Exception) {}
                Log.i(Relay.TAG, "released mobile data")
            }
            callback = null
            cellular = null
        }
    }

    /** The network to bind relayed sockets to, or null for Android's default network. */
    fun network(): Network? = if (callback != null) cellular else null

    /** True when mobile data is wanted but isn't available (traffic uses the default network). */
    fun mobileMissing(): Boolean = callback != null && cellular == null

    /** What the relay is actually using, for the UI and for the PC's flyout. */
    fun effective(context: Context): NetInfo {
        if (network() != null) {
            val carrier = context.getSystemService(TelephonyManager::class.java)?.networkOperatorName.orEmpty()
            return NetInfo(NetInfo.Kind.Mobile, carrier)
        }
        return NetInfo.current(context)
    }
}
