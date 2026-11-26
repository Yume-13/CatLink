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

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

// Blue primary, aqua tertiary, cool neutral surfaces.
private val Light = lightColorScheme(
    primary = Color(0xFF1F63C4),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFD8E5FF),
    onPrimaryContainer = Color(0xFF001A41),
    secondary = Color(0xFF545F71),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFD8E3F8),
    onSecondaryContainer = Color(0xFF111C2B),
    tertiary = Color(0xFF006C4B),
    onTertiary = Color.White,
    tertiaryContainer = Color(0xFF8BF7C4),
    onTertiaryContainer = Color(0xFF002114),
    background = Color(0xFFF8F9FF),
    onBackground = Color(0xFF191C20),
    surface = Color(0xFFF8F9FF),
    onSurface = Color(0xFF191C20),
    surfaceVariant = Color(0xFFE0E2EC),
    onSurfaceVariant = Color(0xFF43474E),
    outline = Color(0xFF74777F),
    outlineVariant = Color(0xFFC4C6D0),
    surfaceContainerLowest = Color.White,
    surfaceContainerLow = Color(0xFFF2F3FA),
    surfaceContainer = Color(0xFFECEEF4),
    surfaceContainerHigh = Color(0xFFE6E8EE),
    surfaceContainerHighest = Color(0xFFE1E2E8),
    error = Color(0xFFBA1A1A),
    onError = Color.White,
)

private val Dark = darkColorScheme(
    primary = Color(0xFFADC6FF),
    onPrimary = Color(0xFF002E69),
    primaryContainer = Color(0xFF004494),
    onPrimaryContainer = Color(0xFFD8E5FF),
    secondary = Color(0xFFBCC7DB),
    onSecondary = Color(0xFF263141),
    secondaryContainer = Color(0xFF3C4758),
    onSecondaryContainer = Color(0xFFD8E3F8),
    tertiary = Color(0xFF6EDAA9),
    onTertiary = Color(0xFF003825),
    tertiaryContainer = Color(0xFF005138),
    onTertiaryContainer = Color(0xFF8BF7C4),
    background = Color(0xFF111318),
    onBackground = Color(0xFFE1E2E9),
    surface = Color(0xFF111318),
    onSurface = Color(0xFFE1E2E9),
    surfaceVariant = Color(0xFF43474E),
    onSurfaceVariant = Color(0xFFC4C6D0),
    outline = Color(0xFF8E9099),
    outlineVariant = Color(0xFF43474E),
    surfaceContainerLowest = Color(0xFF0C0E13),
    surfaceContainerLow = Color(0xFF191C20),
    surfaceContainer = Color(0xFF1D2024),
    surfaceContainerHigh = Color(0xFF282A2F),
    surfaceContainerHighest = Color(0xFF33353A),
    error = Color(0xFFFFB4AB),
    onError = Color(0xFF690005),
)

/** Chart series and status colors. */
@Immutable
data class Extra(
    val download: Color,
    val upload: Color,
    val good: Color,
    val warning: Color,
    val critical: Color,
    val onWarning: Color,
)

private val LightExtra = Extra(
    download = Color(0xFF2A78D6), upload = Color(0xFF1BAF7A),
    good = Color(0xFF0CA30C), warning = Color(0xFFFAB219), critical = Color(0xFFD03B3B), onWarning = Color(0xFF1B1300),
)
private val DarkExtra = Extra(
    download = Color(0xFF3987E5), upload = Color(0xFF199E70),
    good = Color(0xFF0CA30C), warning = Color(0xFFFAB219), critical = Color(0xFFD03B3B), onWarning = Color(0xFF1B1300),
)

val LocalExtra = staticCompositionLocalOf { LightExtra }

private val AppTypography = Typography().let { t ->
    t.copy(
        headlineSmall = t.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
        titleLarge = t.titleLarge.copy(fontWeight = FontWeight.SemiBold),
        titleMedium = t.titleMedium.copy(fontWeight = FontWeight.SemiBold),
        labelLarge = t.labelLarge.copy(fontWeight = FontWeight.SemiBold),
    )
}

val NumberStyle = TextStyle(fontSize = 26.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-0.5).sp)

@Composable
fun CatLinkTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val scheme: ColorScheme = if (dark) Dark else Light
    androidx.compose.runtime.CompositionLocalProvider(LocalExtra provides if (dark) DarkExtra else LightExtra) {
        MaterialTheme(colorScheme = scheme, typography = AppTypography, content = content)
    }
}
