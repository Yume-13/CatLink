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

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import app.catlink.Rate
import app.catlink.RelayStats

/** Graph ceiling: the smallest fixed step (64 KB/s, 128 KB/s, ... 1 GB/s) that is >= the value. */
fun niceMax(v: Float): Float {
    val k = 1024f
    val m = k * k
    return listOf(64 * k, 128 * k, 256 * k, 512 * k, m, 2 * m, 5 * m, 10 * m, 20 * m, 50 * m, 100 * m, 200 * m, 500 * m, 1024 * m)
        .firstOrNull { it >= v } ?: v
}

/**
 * Last 60 s of download/upload on one shared axis. Newest sample on the right.
 * Touch and drag to scrub; [onScrub] receives the history index (null on release).
 */
@Composable
fun ThroughputGraph(
    history: List<Rate>,
    yMax: Float,
    scrub: Int?,
    onScrub: (Int?) -> Unit,
    download: Color,
    upload: Color,
    grid: Color,
    baseline: Color,
    crosshair: Color,
    surface: Color,
    modifier: Modifier = Modifier,
) {
    val slots = RelayStats.HISTORY
    val n = history.size
    Canvas(
        modifier.pointerInput(n) {
            if (n == 0) return@pointerInput
            fun index(x: Float): Int {
                val step = size.width / (slots - 1).toFloat()
                val slot = (x / step).toInt().coerceIn(0, slots - 1)
                return (slot - (slots - n)).coerceIn(0, n - 1)
            }
            awaitEachGesture {
                val down = awaitFirstDown()
                onScrub(index(down.position.x))
                down.consume()
                while (true) {
                    val ev = awaitPointerEvent()
                    val c = ev.changes.firstOrNull() ?: break
                    if (!c.pressed) break
                    onScrub(index(c.position.x))
                    c.consume()
                }
                onScrub(null)
            }
        }
    ) {
        val w = size.width
        val h = size.height
        val hair = 1.dp.toPx()
        // Grid lines: top, middle, baseline.
        if (n > 0) {
            drawLine(grid, Offset(0f, hair / 2), Offset(w, hair / 2), hair)
            drawLine(grid, Offset(0f, h / 2), Offset(w, h / 2), hair)
        }
        drawLine(baseline, Offset(0f, h - hair / 2), Offset(w, h - hair / 2), hair)
        if (n < 2) return@Canvas

        val step = w / (slots - 1).toFloat()
        fun x(i: Int) = (slots - n + i) * step
        fun y(v: Float) = h - (v / yMax).coerceIn(0f, 1f) * (h - hair)

        for ((color, pick) in listOf(download to { r: Rate -> r.down }, upload to { r: Rate -> r.up })) {
            val line = Path()
            val area = Path().apply { moveTo(x(0), h) }
            history.forEachIndexed { i, r ->
                val px = x(i)
                val py = y(pick(r))
                if (i == 0) line.moveTo(px, py) else line.lineTo(px, py)
                area.lineTo(px, py)
            }
            area.lineTo(x(n - 1), h)
            area.close()
            drawPath(area, color.copy(alpha = 0.10f))
            drawPath(line, color, style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
        }

        scrub?.takeIf { it in 0 until n }?.let { i ->
            val px = x(i)
            drawLine(crosshair, Offset(px, 0f), Offset(px, h), hair)
            for ((color, v) in listOf(download to history[i].down, upload to history[i].up)) {
                val c = Offset(px, y(v))
                drawCircle(surface, radius = 6.dp.toPx(), center = c)
                drawCircle(color, radius = 4.dp.toPx(), center = c)
            }
        }
    }
}
