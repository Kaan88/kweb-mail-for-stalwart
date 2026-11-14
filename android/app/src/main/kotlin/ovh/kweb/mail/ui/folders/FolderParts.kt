package ovh.kweb.mail.ui.folders

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerEventTimeoutCancellationException
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.onLongClick
import androidx.compose.ui.semantics.semantics
import ovh.kweb.mail.R
import uniffi.kweb_core.Mailbox

/**
 * The menu of one folder: the same list in the folder screen and in the drawer (a long press). [withBell] adds the
 * bell's entry, which the folder screen has as a button of its own.
 */
@Composable
fun FolderMenu(folder: Mailbox, withBell: Boolean, expanded: Boolean, onDismiss: () -> Unit, extra: (@Composable () -> Unit)? = null, onPick: (FolderMenuItem) -> Unit) {
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        for (item in FolderRules.menu(folder, withBell)) {
            val text = when (item) {
                FolderMenuItem.NOTIFY_ON -> R.string.folders_notify_on
                FolderMenuItem.NOTIFY_OFF -> R.string.folders_notify_off
                FolderMenuItem.RENAME -> R.string.folders_rename
                FolderMenuItem.MOVE -> R.string.folders_move
                FolderMenuItem.NEW_INSIDE -> R.string.folders_new_inside
                FolderMenuItem.DELETE -> R.string.folders_delete
            }
            DropdownMenuItem(text = { Text(stringResource(text)) }, onClick = { onDismiss(); onPick(item) })
        }
        extra?.invoke()
    }
}

/**
 * A long press on this element calls [onLongPress] and keeps the element's own click from firing for that touch. It
 * looks at the touch before the element does (the initial pass), because an element with a click of its own, such as
 * a drawer item, would take the release otherwise.
 */
@Composable
fun Modifier.longPressMenu(label: String, onLongPress: () -> Unit): Modifier {
    val timeout = LocalViewConfiguration.current.longPressTimeoutMillis
    return this
        .semantics { onLongClick(label) { onLongPress(); true } }
        .pointerInput(onLongPress, timeout) {
            awaitEachGesture {
                awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                try {
                    withTimeout(timeout) { waitForUpOrCancellation(PointerEventPass.Initial) }
                } catch (_: PointerEventTimeoutCancellationException) {
                    onLongPress()
                    // The rest of this touch is ours: the element below must not see its release as a click.
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Initial)
                        event.changes.forEach { it.consume() }
                        if (event.changes.none { it.pressed }) break
                    }
                }
            }
        }
}
