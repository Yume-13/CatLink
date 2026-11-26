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
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/** Sitting cat with a curled tail (11 x 11). `#` is the body, `o` the eyes. Same sprite as the PC tray app. */
private val CAT = listOf(
    "#.....#....",
    "##...##....",
    "#######....",
    "##o#o##....",
    "#######....",
    ".#####.....",
    ".#####....#",
    "#######...#",
    "#######...#",
    "#######..#.",
    ".########..",
)

/** Pixel-art cat. Each sprite pixel is drawn as a whole number of screen pixels. */
@Composable
fun PixelCat(body: Color, eyes: Color, modifier: Modifier = Modifier, pixel: Dp = 3.dp) {
    Canvas(modifier.size(pixel * CAT[0].length, pixel * CAT.size)) {
        val px = pixel.toPx().roundToInt().coerceAtLeast(1).toFloat()
        val cell = Size(px, px)
        CAT.forEachIndexed { row, line ->
            line.forEachIndexed { col, ch ->
                val color = when (ch) {
                    '#' -> body
                    'o' -> eyes
                    else -> return@forEachIndexed
                }
                drawRect(color, Offset(col * px, row * px), cell)
            }
        }
    }
}
