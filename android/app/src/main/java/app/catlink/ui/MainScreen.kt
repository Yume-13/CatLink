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

package app.catlink.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.DeveloperMode
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.LinkOff
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.SignalCellularAlt
import androidx.compose.material.icons.rounded.SignalWifi4Bar
import androidx.compose.material.icons.rounded.SignalWifiOff
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material.icons.rounded.Usb
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.catlink.Fmt
import app.catlink.NetInfo
import app.catlink.Rate

/** Everything the screen shows, captured once per second. */
data class UiState(
    val running: Boolean,
    val error: String?,
    val pcConnected: Boolean,
    val connectedForMs: Long,
    val history: List<Rate>,
    val openConnections: Long,
    val bytesDown: Long,
    val bytesUp: Long,
    val net: NetInfo,
    val usbDebugging: Boolean,
    /** Developer options › Wireless debugging (Android 11+). */
    val wirelessDebugging: Boolean,
    /** The PC is connected over Wi-Fi. */
    val wirelessLink: Boolean,
    /** Setting: send traffic over mobile data when the PC is connected over Wi-Fi. */
    val mobileOverWifi: Boolean,
    /** Mobile data is wanted but not available, so traffic uses the default network. */
    val mobileMissing: Boolean,
    /** null when the permission doesn't exist (Android 12 and older). */
    val notificationsAllowed: Boolean?,
)

private enum class Phase { Off, Waiting, Sharing, Failed }

private val UiState.phase: Phase
    get() = when {
        error != null -> Phase.Failed
        !running -> Phase.Off
        pcConnected -> Phase.Sharing
        else -> Phase.Waiting
    }

@Composable
fun MainScreen(
    state: UiState,
    onStart: () -> Unit,
    onStop: () -> Unit,
    onOpenDeveloperSettings: () -> Unit,
    onAllowNotifications: () -> Unit,
    onMobileOverWifi: (Boolean) -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .systemBarsPadding()
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Header()
            HeroCard(state, onStart, onStop)
            if (state.running) {
                ThroughputCard(state)
                StatsRow(state)
            }
            SetupCard(state, onOpenDeveloperSettings, onAllowNotifications)
            OptionsCard(state, onMobileOverWifi)
            Text(
                "Connections from your PC are opened by CatLink itself, so Android counts them as this app’s own data rather than a hotspot.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun Header() {
    Column(Modifier.padding(horizontal = 4.dp, vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.Bottom) {
            Text("CatLink", style = MaterialTheme.typography.headlineMedium, color = MaterialTheme.colorScheme.onBackground)
            PixelCat(
                body = MaterialTheme.colorScheme.onSurfaceVariant,
                eyes = MaterialTheme.colorScheme.tertiary,
                modifier = Modifier.padding(start = 12.dp, bottom = 7.dp),
            )
        }
        Text(
            "Share this phone’s internet with your PC over USB or Wi-Fi",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun SectionCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(24.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) { content() }
}

@Composable
private fun HeroCard(state: UiState, onStart: () -> Unit, onStop: () -> Unit) {
    val x = LocalExtra.current
    val phase = state.phase
    val (color, onColor) = when (phase) {
        Phase.Sharing -> x.good to Color.White
        Phase.Waiting -> x.warning to x.onWarning
        Phase.Failed -> x.critical to Color.White
        Phase.Off -> MaterialTheme.colorScheme.outline to MaterialTheme.colorScheme.surface
    }
    val icon = when (phase) {
        Phase.Sharing -> Icons.Rounded.Link
        Phase.Waiting -> Icons.Rounded.Usb
        Phase.Failed -> Icons.Rounded.ErrorOutline
        Phase.Off -> Icons.Rounded.LinkOff
    }
    val title = when (phase) {
        Phase.Sharing -> "Sharing with your PC"
        Phase.Waiting -> "Waiting for your PC"
        Phase.Failed -> "Couldn’t start sharing"
        Phase.Off -> "Not sharing"
    }
    val subtitle = when (phase) {
        Phase.Sharing -> buildList {
            add("Via ${state.net.kind.label.lowercase().replace("wi-fi", "Wi-Fi")}")
            if (state.net.carrier.isNotBlank()) add(state.net.carrier)
            add(if (state.wirelessLink) "PC on Wi-Fi" else "PC on USB")
            add("for ${Fmt.duration(state.connectedForMs)}")
        }.joinToString(" · ")
        Phase.Waiting -> "Plug in the USB cable (or pair over Wi-Fi) and run CatLink on the PC. It connects automatically."
        Phase.Failed -> state.error ?: ""
        Phase.Off -> "Start sharing, then run CatLink on your PC."
    }
    SectionCard {
        Column(
            Modifier.fillMaxWidth().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            StatusOrb(color, onColor, icon, pulsing = phase == Phase.Sharing || phase == Phase.Waiting, fast = phase == Phase.Sharing)
            Spacer(Modifier.height(16.dp))
            Text(title, style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.onSurface, textAlign = TextAlign.Center)
            Spacer(Modifier.height(4.dp))
            Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
            Spacer(Modifier.height(20.dp))
            if (state.running) {
                FilledTonalButton(onClick = onStop, modifier = Modifier.fillMaxWidth().height(56.dp), shape = RoundedCornerShape(16.dp)) {
                    Icon(Icons.Rounded.Stop, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("Stop sharing", style = MaterialTheme.typography.labelLarge)
                }
            } else {
                Button(
                    onClick = onStart,
                    modifier = Modifier.fillMaxWidth().height(56.dp),
                    shape = RoundedCornerShape(16.dp),
                    colors = ButtonDefaults.buttonColors(),
                ) {
                    Icon(Icons.Rounded.PlayArrow, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("Start sharing", style = MaterialTheme.typography.labelLarge)
                }
            }
        }
    }
}

@Composable
private fun StatusOrb(color: Color, onColor: Color, icon: ImageVector, pulsing: Boolean, fast: Boolean) {
    val animated by animateColorAsState(color, label = "orb")
    val t = rememberInfiniteTransition(label = "pulse")
    val progress by t.animateFloat(
        0f, 1f,
        infiniteRepeatable(tween(if (fast) 1600 else 2400, easing = LinearEasing), RepeatMode.Restart),
        label = "p",
    )
    Box(Modifier.size(120.dp), contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(120.dp)) {
            val r0 = 32.dp.toPx()
            val r1 = size.minDimension / 2
            drawCircle(animated.copy(alpha = 0.12f), radius = 46.dp.toPx())
            if (pulsing) {
                for (k in 0..1) {
                    val p = (progress + k * 0.5f) % 1f
                    drawCircle(
                        animated.copy(alpha = 0.45f * (1 - p)),
                        radius = r0 + (r1 - r0) * p,
                        style = Stroke(width = 2.dp.toPx()),
                    )
                }
            }
        }
        Box(
            Modifier.size(64.dp).clip(CircleShape).background(animated),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = null, tint = onColor, modifier = Modifier.size(32.dp))
        }
    }
}

@Composable
private fun ThroughputCard(state: UiState) {
    val x = LocalExtra.current
    val cs = MaterialTheme.colorScheme
    var scrub by remember { mutableStateOf<Int?>(null) }
    val h = state.history
    val shown = scrub?.takeIf { it in h.indices } ?: h.lastIndex.takeIf { it >= 0 }
    val sample = shown?.let { h[it] }
    val yMax = niceMax(h.maxOfOrNull { maxOf(it.down, it.up) } ?: 0f)
    SectionCard {
        Column(Modifier.padding(20.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Throughput", style = MaterialTheme.typography.titleMedium, color = cs.onSurface, modifier = Modifier.weight(1f))
                val label = when {
                    scrub != null && shown != null -> (h.lastIndex - shown).let { if (it == 0) "Now" else "$it s ago" }
                    else -> "Live"
                }
                Text(label, style = MaterialTheme.typography.labelMedium, color = cs.onSurfaceVariant)
            }
            Spacer(Modifier.height(12.dp))
            Row {
                Metric("Download", x.download, sample?.let { Fmt.rate(it.down) }, Modifier.weight(1f))
                Metric("Upload", x.upload, sample?.let { Fmt.rate(it.up) }, Modifier.weight(1f))
            }
            Spacer(Modifier.height(16.dp))
            Text(
                if (h.isEmpty()) "" else Fmt.rate(yMax).replace(".0 ", " "),
                style = MaterialTheme.typography.labelSmall,
                color = cs.onSurfaceVariant,
            )
            Spacer(Modifier.height(4.dp))
            Box(Modifier.fillMaxWidth().height(120.dp), contentAlignment = Alignment.Center) {
                ThroughputGraph(
                    history = h,
                    yMax = yMax,
                    scrub = scrub,
                    onScrub = { scrub = it },
                    download = x.download,
                    upload = x.upload,
                    grid = cs.outlineVariant.copy(alpha = 0.6f),
                    baseline = cs.outlineVariant,
                    crosshair = cs.onSurfaceVariant,
                    surface = cs.surfaceContainer,
                    modifier = Modifier.fillMaxSize(),
                )
                if (h.size < 2) {
                    Text("Waiting for traffic…", style = MaterialTheme.typography.bodySmall, color = cs.onSurfaceVariant)
                }
            }
            Spacer(Modifier.height(6.dp))
            Row {
                Text("60 s ago", style = MaterialTheme.typography.labelSmall, color = cs.onSurfaceVariant, modifier = Modifier.weight(1f))
                Text("Now", style = MaterialTheme.typography.labelSmall, color = cs.onSurfaceVariant)
            }
        }
    }
}

@Composable
private fun Metric(label: String, swatch: Color, value: String?, modifier: Modifier = Modifier) {
    Column(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(10.dp).clip(RoundedCornerShape(2.dp)).background(swatch))
            Spacer(Modifier.width(6.dp))
            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(2.dp))
        Text(value ?: "—", style = NumberStyle, color = MaterialTheme.colorScheme.onSurface)
    }
}

@Composable
private fun StatsRow(state: UiState) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        StatTile("Active", if (state.pcConnected) "${state.openConnections} open" else "—", Modifier.weight(1f))
        StatTile("Received", Fmt.bytes(state.bytesDown), Modifier.weight(1f))
        StatTile("Sent", Fmt.bytes(state.bytesUp), Modifier.weight(1f))
    }
}

@Composable
private fun StatTile(label: String, value: String, modifier: Modifier) {
    Card(
        modifier = modifier,
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) {
        Column(Modifier.padding(horizontal = 14.dp, vertical = 12.dp)) {
            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            Spacer(Modifier.height(2.dp))
            Text(value, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurface, maxLines = 1)
        }
    }
}

@Composable
private fun SetupCard(state: UiState, onOpenDeveloperSettings: () -> Unit, onAllowNotifications: () -> Unit) {
    SectionCard {
        Column(Modifier.padding(vertical = 8.dp)) {
            Text(
                "Setup",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 12.dp, bottom = 4.dp),
            )
            val debugging = state.usbDebugging || state.wirelessDebugging
            SetupRow(
                Icons.Rounded.DeveloperMode,
                "USB or wireless debugging",
                when {
                    state.usbDebugging && state.wirelessDebugging -> "USB and wireless debugging are on"
                    state.usbDebugging -> "USB debugging is on"
                    state.wirelessDebugging -> "Wireless debugging is on"
                    else -> "Needed so the PC can reach this app"
                },
                done = debugging,
                action = if (debugging) null else ("Open settings" to onOpenDeveloperSettings),
            )
            Divider()
            val netIcon = when (state.net.kind) {
                NetInfo.Kind.WiFi -> Icons.Rounded.SignalWifi4Bar
                NetInfo.Kind.None -> Icons.Rounded.SignalWifiOff
                else -> Icons.Rounded.SignalCellularAlt
            }
            val netText = when (state.net.kind) {
                NetInfo.Kind.None -> "This phone has no internet right now"
                else -> listOf(state.net.kind.label, state.net.carrier).filter { it.isNotBlank() }.joinToString(" · ")
            }
            SetupRow(netIcon, "Phone connection", netText, done = state.net.kind != NetInfo.Kind.None, action = null)
            Divider()
            SetupRow(
                Icons.Rounded.Computer,
                "CatLink on your PC",
                if (state.pcConnected) "Connected" else "Run catlink.exe; it starts this app by itself",
                done = state.pcConnected,
                action = null,
            )
            state.notificationsAllowed?.let { allowed ->
                Divider()
                SetupRow(
                    Icons.Rounded.Notifications,
                    "Notifications",
                    if (allowed) "Live speed in the notification shade" else "Optional: show live speed while sharing",
                    done = allowed,
                    action = if (allowed) null else ("Allow" to onAllowNotifications),
                )
            }
        }
    }
}

@Composable
private fun OptionsCard(state: UiState, onMobileOverWifi: (Boolean) -> Unit) {
    SectionCard {
        Column(Modifier.padding(vertical = 8.dp)) {
            Text(
                "Options",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 12.dp, bottom = 4.dp),
            )
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { onMobileOverWifi(!state.mobileOverWifi) }
                    .padding(horizontal = 20.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier.size(36.dp).clip(CircleShape).background(MaterialTheme.colorScheme.secondaryContainer),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.Rounded.SignalCellularAlt,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSecondaryContainer,
                        modifier = Modifier.size(20.dp),
                    )
                }
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f)) {
                    Text("Use mobile data over Wi-Fi", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface)
                    Text(
                        when {
                            state.mobileOverWifi && state.mobileMissing ->
                                "Mobile data isn’t available, so your PC’s traffic is going over Wi-Fi."
                            else ->
                                "When your PC connects over wireless debugging, send its traffic over mobile data instead of Wi-Fi."
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = if (state.mobileOverWifi && state.mobileMissing) LocalExtra.current.critical else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(12.dp))
                Switch(checked = state.mobileOverWifi, onCheckedChange = onMobileOverWifi)
            }
        }
    }
}

@Composable
private fun Divider() {
    HorizontalDivider(Modifier.padding(start = 72.dp, end = 20.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.6f))
}

@Composable
private fun SetupRow(icon: ImageVector, title: String, subtitle: String, done: Boolean, action: Pair<String, () -> Unit>?) {
    val x = LocalExtra.current
    Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier.size(36.dp).clip(CircleShape).background(MaterialTheme.colorScheme.secondaryContainer),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onSecondaryContainer, modifier = Modifier.size(20.dp))
        }
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface)
            Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        when {
            action != null -> TextButton(onClick = action.second) { Text(action.first) }
            done -> Icon(Icons.Rounded.CheckCircle, contentDescription = "Done", tint = x.good, modifier = Modifier.size(22.dp))
        }
    }
}
