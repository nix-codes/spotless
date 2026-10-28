package dev.spotless.ui

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.lazy.LazyListState
import kotlin.math.abs
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sign

/*
 * The glide to the playing song, as on the desktop: a scroll that speeds up
 * and slows down again onto the song. Further than a few screens, it cuts
 * to that far off first. A finger on the list stops it where it is.
 */

/** The longest a glide goes, in screens. */
private const val SCREENS = 3f
/** How long a glide takes, in milliseconds, from the shortest to the longest. */
private const val SHORTEST = 250
private const val LONGEST = 500
private val EASE = CubicBezierEasing(0.65f, 0f, 0.35f, 1f)

/** Glides until item `index` is at the top, or as near as the list goes. */
suspend fun LazyListState.glideTo(index: Int) {
    val screen = layoutInfo.viewportSize.height.toFloat()
    val far = SCREENS * screen
    var distance = distanceTo(index)
    if (abs(distance) > far) {
        scrollToItem(index)
        distance = -scrollBy(-sign(distance) * far)
    }
    val share = min(abs(distance) / far.coerceAtLeast(1f), 1f)
    val time = SHORTEST + ((LONGEST - SHORTEST) * share).roundToInt()
    animateScrollBy(distance, tween(time, easing = EASE))
    // Off-screen items are sized by guess: the last pixels come right here.
    scrollToItem(index)
}

/** How far down item `index` is, in pixels: exact if in sight, else guessed. */
private fun LazyListState.distanceTo(index: Int): Float {
    val shown = layoutInfo.visibleItemsInfo
    shown.firstOrNull { it.index == index }?.let { return it.offset.toFloat() }
    val first = shown.firstOrNull() ?: return 0f
    val size = shown.sumOf { it.size }.toFloat() / shown.size
    return (index - first.index) * size + first.offset
}
