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

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import app.catlink.ui.CatLinkTheme
import app.catlink.ui.MainScreen
import app.catlink.ui.UiState
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        handleIntent(intent)
        setContent {
            CatLinkTheme {
                var tick by remember { mutableIntStateOf(0) }
                LaunchedEffect(Unit) {
                    while (true) {
                        delay(1000)
                        tick++
                    }
                }
                val notifLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { tick++ }
                val state = remember(tick) { snapshot() }
                MainScreen(
                    state = state,
                    onStart = { RelayService.start(this); refreshSoon { tick++ } },
                    onStop = { RelayService.stop(this); refreshSoon { tick++ } },
                    onOpenDeveloperSettings = {
                        try {
                            startActivity(Intent(Settings.ACTION_APPLICATION_DEVELOPMENT_SETTINGS))
                        } catch (_: Exception) {
                            startActivity(Intent(Settings.ACTION_DEVICE_INFO_SETTINGS))
                        }
                    },
                    onAllowNotifications = {
                        if (Build.VERSION.SDK_INT >= 33) notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                    },
                    onMobileOverWifi = { Uplink.setMobileOverWifi(this, it); tick++ },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    /** Starts the relay on the START action, which the Windows client sends through adb. */
    private fun handleIntent(intent: Intent?) {
        if (intent?.action == "app.catlink.action.START" && !RelayService.running) RelayService.start(this)
    }

    private fun refreshSoon(refresh: () -> Unit) {
        window.decorView.postDelayed(refresh, 250)
    }

    private fun snapshot(): UiState {
        val notif = if (Build.VERSION.SDK_INT >= 33) {
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        } else null
        val pc = RelayService.running && RelayStats.pcConnected()
        return UiState(
            running = RelayService.running,
            error = RelayService.lastError,
            pcConnected = pc,
            connectedForMs = if (pc) System.currentTimeMillis() - RelayStats.peerSince else 0,
            history = RelayStats.history,
            openConnections = RelayStats.active.get(),
            bytesDown = RelayStats.bytesDown.get(),
            bytesUp = RelayStats.bytesUp.get(),
            net = if (RelayService.running) Uplink.effective(this) else NetInfo.current(this),
            usbDebugging = Settings.Global.getInt(contentResolver, Settings.Global.ADB_ENABLED, 0) == 1,
            // Key of the hidden Settings.Global.ADB_WIFI_ENABLED.
            wirelessDebugging = Settings.Global.getInt(contentResolver, "adb_wifi_enabled", 0) == 1,
            wirelessLink = pc && Uplink.wirelessLink(),
            mobileOverWifi = Uplink.mobileOverWifi(this),
            mobileMissing = Uplink.mobileMissing(),
            notificationsAllowed = notif,
        )
    }
}
