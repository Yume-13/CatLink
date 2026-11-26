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

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.TimeUnit

/** Foreground service that keeps the relay alive while the screen is off. */
class RelayService : Service() {
    companion object {
        private const val CHANNEL = "relay"
        private const val NOTIFICATION_ID = 1
        const val ACTION_STOP = "app.catlink.action.STOP"

        @Volatile var running = false
            private set
        @Volatile var lastError: String? = null

        fun start(context: Context) {
            lastError = null
            context.startForegroundService(Intent(context, RelayService::class.java))
        }

        fun stop(context: Context) {
            context.startService(Intent(context, RelayService::class.java).setAction(ACTION_STOP))
        }
    }

    private var relay: Relay? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var sampler: ScheduledExecutorService? = null
    private var lastNotificationText: String? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        startForegroundCompat(buildNotification())
        if (relay == null) {
            try {
                RelayStats.reset()
                relay = Relay(this).also { it.start() }
                running = true
                lastError = null
                wakeLock = getSystemService(PowerManager::class.java)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "CatLink::relay")
                    .apply { setReferenceCounted(false); acquire() }
                var tick = 0
                sampler = Executors.newSingleThreadScheduledExecutor().apply {
                    scheduleAtFixedRate({
                        RelayStats.sample()
                        Uplink.update(this@RelayService, true)
                        if (tick++ % 2 == 0) refreshNotification()
                    }, 1, 1, TimeUnit.SECONDS)
                }
            } catch (e: Exception) {
                Log.e(Relay.TAG, "could not start relay", e)
                lastError = e.message ?: e.toString()
                stopSelf()
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        sampler?.shutdownNow()
        sampler = null
        relay?.stop()
        relay = null
        running = false
        Uplink.update(this, false)
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        super.onDestroy()
    }

    private fun refreshNotification() {
        val (title, text) = notificationText()
        if (title + text == lastNotificationText) return
        try {
            getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
        } catch (_: Exception) {
        }
    }

    private fun notificationText(): Pair<String, String> =
        if (RelayStats.pcConnected()) {
            val r = RelayStats.history.lastOrNull() ?: Rate(0f, 0f)
            getString(R.string.notif_title_connected) to getString(
                R.string.notif_text_connected, Fmt.rate(r.down), Fmt.rate(r.up),
                RelayStats.active.get().toInt().let { resources.getQuantityString(R.plurals.connections, it, it) }
            )
        } else {
            getString(R.string.notif_title_waiting) to getString(R.string.notif_text_waiting)
        }

    private fun buildNotification(): Notification {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.channel_name), NotificationManager.IMPORTANCE_LOW)
        )
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, RelayService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE
        )
        val (title, text) = notificationText()
        lastNotificationText = title + text
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_link)
            .setColor(getColor(R.color.brand))
            .setContentTitle(title)
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .addAction(
                Notification.Action.Builder(
                    Icon.createWithResource(this, R.drawable.ic_stat_link), getString(R.string.stop), stop
                ).build()
            )
            .build()
    }

    private fun startForegroundCompat(n: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(NOTIFICATION_ID, n)
        }
    }
}
