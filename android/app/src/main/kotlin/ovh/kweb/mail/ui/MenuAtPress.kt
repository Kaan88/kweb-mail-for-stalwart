package ovh.kweb.mail.ui

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.absoluteOffset
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.AbsoluteAlignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.layout
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.round

/**
 * Where a menu opened by a long press should hang: the point the finger went down on, in the pixels of the element that
 * was pressed. A DropdownMenu anchors at the bottom-left corner of its parent, which for a full-width row is the far
 * edge of the screen from the finger; [AtPress] gives it a parent of no size at this point instead.
 */
@Stable
class PressPoint {
    var at by mutableStateOf(IntOffset.Zero)
        private set
    private var width = 0
    private var height = 0
    private var rtl = false

    /** Whether a finger is on the element now: a long press without one came from TalkBack or a keyboard. */
    private var down = false

    internal fun pressed(position: Offset) {
        at = position.round()
        down = true
    }

    internal fun released() {
        down = false
    }

    internal fun sized(width: Int, height: Int, rtl: Boolean) {
        this.width = width
        this.height = height
        this.rtl = rtl
    }

    /**
     * No finger (TalkBack's long-press action, a keyboard): the menu hangs below the element's start, as before. The
     * point is counted from the left like the finger's, so in a right-to-left layout the start is the right edge.
     */
    fun underStart() {
        at = IntOffset(if (rtl) width else 0, height)
    }

    /** For a long-press callback that serves both a finger and the accessibility action: [underStart] when no finger is down. */
    fun settle() {
        if (!down) underStart()
    }
}

@Composable
fun rememberPressPoint(): PressPoint = remember { PressPoint() }

/**
 * Notes where each touch on this element goes down, without taking the touch: the element's own click and long press
 * work as before. It looks before the element does (the initial pass), so a child that takes the touch does not hide it.
 */
fun Modifier.notePress(point: PressPoint): Modifier = this
    .layout { measurable, constraints ->
        val placeable = measurable.measure(constraints)
        point.sized(placeable.width, placeable.height, layoutDirection == LayoutDirection.Rtl)
        layout(placeable.width, placeable.height) { placeable.place(0, 0) }
    }
    .pointerInput(point) {
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            point.pressed(down.position)
            try {
                var stillDown = true
                while (stillDown) stillDown = awaitPointerEvent(PointerEventPass.Initial).changes.any { it.pressed }
            } finally {
                point.released()
            }
        }
    }

/**
 * A point of no size at [point], for a DropdownMenu in [content] to hang from: the menu opens at the finger, below it, or
 * above it when there is no room below, and it is moved in from the screen's edge as the menu always is. Put it in the
 * same Box as the pressed element, with that element filling the Box's width from its top, so their pixels agree. The
 * point is counted from the Box's left edge, as the finger's position is, in either layout direction.
 */
@Composable
fun BoxScope.AtPress(point: PressPoint, content: @Composable () -> Unit) {
    Box(Modifier.align(AbsoluteAlignment.TopLeft).absoluteOffset { point.at }.size(0.dp)) { content() }
}
