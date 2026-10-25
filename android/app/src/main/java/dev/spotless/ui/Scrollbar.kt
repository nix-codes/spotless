package dev.spotless.ui

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/*
 * The scrollbar, Winamp's playlist one as on the desktop: a gold thumb in a
 * slot of its own beside what scrolls, there only when there is more than
 * fits. Dragging it scrolls; pressing in the slot brings the thumb's middle
 * to the finger, and it drags on from there.
 */

private val WIDTH = 16.dp
private val THUMB = 40.dp
private val EDGE = 2.dp

/** A lazy list with the scrollbar at its right. */
@Composable
fun ScrollingList(state: LazyListState, modifier: Modifier = Modifier, content: LazyListScope.() -> Unit) {
    Row(modifier) {
        LazyColumn(Modifier.weight(1f).fillMaxHeight(), state, content = content)
        Scrollbar(state)
    }
}

/** The scrollbar of a lazy list, whose rows may differ in height: it goes by those in sight. */
@Composable
fun Scrollbar(state: LazyListState, modifier: Modifier = Modifier) {
    val scope = rememberCoroutineScope()
    // How tall a row is on average, how tall all of them would be, and how far down the list is.
    val measure by remember(state) {
        derivedStateOf {
            val info = state.layoutInfo
            val shown = info.visibleItemsInfo
            if (shown.isEmpty()) return@derivedStateOf null
            val row = shown.sumOf { it.size }.toFloat() / shown.size
            val far = row * info.totalItemsCount - (info.viewportEndOffset - info.viewportStartOffset)
            val down = state.firstVisibleItemIndex * row + state.firstVisibleItemScrollOffset
            Triple(row, far, if (state.canScrollForward) (down / far).coerceIn(0f, 1f) else 1f)
        }
    }
    val m = measure
    val scrolls = state.canScrollForward || state.canScrollBackward
    Slot(if (m != null && scrolls) m.third else 0f, modifier, enabled = m != null && scrolls) { at ->
        val (row, far) = measure ?: return@Slot
        val y = at * far.coerceAtLeast(0f)
        val index = (y / row).toInt().coerceIn(0, state.layoutInfo.totalItemsCount - 1)
        scope.launch { state.scrollToItem(index, (y - index * row).toInt()) }
    }
}

/** The scrollbar of a column with `verticalScroll`. */
@Composable
fun Scrollbar(state: ScrollState, modifier: Modifier = Modifier) {
    val scope = rememberCoroutineScope()
    val scrolls = state.maxValue > 0
    Slot(if (scrolls) state.value.toFloat() / state.maxValue else 0f, modifier, enabled = scrolls) { at ->
        scope.launch { state.scrollTo((at * state.maxValue).toInt()) }
    }
}

/**
 * The slot and its thumb at `at` (0 at the top, 1 at the bottom); `scrollTo` gets where it is dragged.
 * Nothing at all when the list doesn't scroll, rather than a bar that suggests more.
 */
@Composable
private fun Slot(at: Float, modifier: Modifier, enabled: Boolean, scrollTo: (Float) -> Unit) {
    if (!enabled) return
    val now by rememberUpdatedState(at)
    val go by rememberUpdatedState(scrollTo)
    val slot = modifier
        .width(WIDTH)
        .fillMaxHeight()
        .pointerInput(Unit) {
            val thumb = THUMB.toPx()
            awaitEachGesture {
                val down = awaitFirstDown()
                val travel = (size.height - thumb).coerceAtLeast(1f)
                val top = now * travel
                // How far below the thumb's top it was taken hold of.
                val grab = if (down.position.y in top..top + thumb) down.position.y - top else thumb / 2
                go(((down.position.y - grab) / travel).coerceIn(0f, 1f))
                down.consume()
                drag(down.id) { change ->
                    change.consume()
                    go(((change.position.y - grab) / travel).coerceIn(0f, 1f))
                }
            }
        }
        .drawBehind {
            val thumb = THUMB.toPx()
            val edge = EDGE.toPx()
            drawRect(Classic.frame)
            bevel(Offset.Zero, size, edge, Classic.lo, Classic.hi)
            val top = at * (size.height - thumb).coerceAtLeast(0f)
            val face = Classic.thumb
            drawRect(face, Offset(0f, top), Size(size.width, thumb))
            bevel(Offset(0f, top), Size(size.width, thumb), edge, lerp(face, Color.White, 0.45f), lerp(face, Color.Black, 0.9f))
        }
    Box(slot)
}

/** Light top and left edges, dark bottom and right: raised, or sunk with the colours swapped. */
private fun DrawScope.bevel(at: Offset, size: Size, w: Float, topLeft: Color, bottomRight: Color) {
    drawRect(topLeft, at, Size(size.width, w))
    drawRect(topLeft, at, Size(w, size.height))
    drawRect(bottomRight, Offset(at.x, at.y + size.height - w), Size(size.width, w))
    drawRect(bottomRight, Offset(at.x + size.width - w, at.y), Size(w, size.height))
}
