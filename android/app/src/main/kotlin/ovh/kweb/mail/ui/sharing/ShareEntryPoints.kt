package ovh.kweb.mail.ui.sharing

import android.app.Application
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.shared.SharedRules
import uniffi.kweb_core.ShareKind

/** What opens the Share screen for one thing of the user's own: the kind, its id and its name (for the title). */
typealias OpenShare = (kind: ShareKind, id: String, name: String) -> Unit

/** Provided once by the app; every "Share" entry (folder, calendar, address book, file folder) calls it. A no-op where nothing provides it. */
val LocalOpenShare = compositionLocalOf<OpenShare> { { _, _, _ -> } }

/**
 * A long press on a row whose own click must keep working (a drawer item has no long-press callback). The row
 * takes [interactionSource] and passes its click through [guard]: a click that ends a long press is swallowed.
 */
class LongPress(val interactionSource: MutableInteractionSource) {
    var fired = false
    var menuOpen by mutableStateOf(false)

    fun guard(onClick: () -> Unit): () -> Unit = {
        if (fired) fired = false else onClick()
    }

    fun dismiss() {
        menuOpen = false
    }
}

/** A long press of [enabled] rows opens [LongPress.menuOpen] once the system's long-press time has passed. */
@Composable
fun rememberLongPress(enabled: Boolean): LongPress {
    val press = remember { LongPress(MutableInteractionSource()) }
    val pressed by press.interactionSource.collectIsPressedAsState()
    // A press that ends without a click (cancelled by a scroll or drag) must not swallow the next tap.
    LaunchedEffect(press) {
        press.interactionSource.interactions.collect { if (it is PressInteraction.Cancel) press.fired = false }
    }
    val timeout = LocalViewConfiguration.current.longPressTimeoutMillis
    LaunchedEffect(pressed, enabled) {
        if (pressed) {
            press.fired = false
            if (enabled) {
                delay(timeout)
                press.fired = true
                press.menuOpen = true
            }
        }
    }
    return press
}

/**
 * The menu a long press on a drawer folder opens. It holds "Share" and has [extra] for the entries of others
 * (put inside the same menu); [shareable] false (Drafts, Trash, Junk) leaves out Share.
 */
@Composable
fun FolderLongPressMenu(press: LongPress, shareable: Boolean, onShare: () -> Unit, extra: @Composable (close: () -> Unit) -> Unit = {}) {
    DropdownMenu(expanded = press.menuOpen, onDismissRequest = press::dismiss) {
        if (shareable) {
            DropdownMenuItem(text = { Text(stringResource(R.string.share_menu)) }, onClick = { press.dismiss(); onShare() })
        }
        extra(press::dismiss)
    }
}

/** The kinds that are chosen from a list of the user's own things. */
enum class PickKind { CALENDAR, ADDRESS_BOOK }

/**
 * A three-dots button whose menu has "Share a calendar…" (or "Share an address book…"); a tap lists the user's own
 * ones to pick from and opens the Share screen for the one picked.
 */
@Composable
fun SharePickerButton(kind: PickKind, modifier: Modifier = Modifier) {
    val open = LocalOpenShare.current
    var menu by remember { mutableStateOf(false) }
    var picking by rememberSaveable { mutableStateOf(false) }
    Box(modifier) {
        IconButton(onClick = { menu = true }) {
            Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.share_more))
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(if (kind == PickKind.CALENDAR) R.string.share_calendar_menu else R.string.share_book_menu)) },
                onClick = { menu = false; picking = true },
            )
        }
    }
    if (picking) {
        SharePickerDialog(
            kind = kind,
            onPick = { shareKind, id, name ->
                picking = false
                open(shareKind, id, name)
            },
            onDismiss = { picking = false },
        )
    }
}

/** What the picker shows: the things, an error, or nothing yet. */
private data class Picked(val items: List<Pair<String, String>>? = null, val error: UiMessage? = null)

@Composable
private fun SharePickerDialog(kind: PickKind, onPick: OpenShare, onDismiss: () -> Unit) {
    val app = LocalContext.current.applicationContext as Application
    var picked by remember { mutableStateOf(Picked()) }
    var attempt by remember { mutableIntStateOf(0) }
    LaunchedEffect(kind, attempt) {
        picked = Picked()
        picked = try {
            val items = withContext(Dispatchers.IO) {
                val backend = CoreSharingBackend { Background.core(app) }
                if (kind == PickKind.CALENDAR) backend.calendars().map { it.id to it.name } else backend.addressBooks().map { it.id to it.name }
            }
            Picked(items = items)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Picked(error = SharedRules.message(e))
        }
    }
    val shareKind = if (kind == PickKind.CALENDAR) ShareKind.CALENDAR else ShareKind.ADDRESS_BOOK
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(if (kind == PickKind.CALENDAR) R.string.share_pick_calendar else R.string.share_pick_book)) },
        text = {
            val items = picked.items
            val error = picked.error
            when {
                items != null && items.isEmpty() -> Text(stringResource(R.string.share_pick_empty))
                items != null -> LazyColumn(Modifier.heightIn(max = 360.dp)) {
                    items(items, key = { it.first }) { (id, name) ->
                        Text(
                            name,
                            modifier = Modifier.fillMaxWidth().clickable { onPick(shareKind, id, name) }.padding(vertical = 12.dp),
                            style = MaterialTheme.typography.bodyLarge,
                        )
                    }
                }
                error != null -> Column {
                    Text(stringResource(R.string.share_pick_failed))
                    Text(error.resolve(androidx.compose.ui.platform.LocalResources.current), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    TextButton(onClick = { attempt++ }) { Text(stringResource(R.string.retry)) }
                }
                else -> CircularProgressIndicator()
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
