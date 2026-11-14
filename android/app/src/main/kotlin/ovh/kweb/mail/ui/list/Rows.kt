package ovh.kweb.mail.ui.list

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.ZoneId
import java.util.Locale
import ovh.kweb.mail.R
import uniffi.kweb_core.Mailbox

/** What a row of a mail list draws. Conversations and search results both become one. */
data class RowContent(
    val sender: String,
    val initial: String,
    val subject: String,
    val preview: String,
    val receivedAt: Long,
    val unread: Boolean,
    val flagged: Boolean,
    val hasAttachment: Boolean,
)

/**
 * One row of the mock-up: unread mail on a raised surface with a square avatar, read mail on a
 * quiet one with a round avatar. [shape] is the row's place in its group ([Groups]).
 * [onLongClick] is null where there is no menu.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun MailRow(
    row: RowContent,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    shape: Shape = RoundedCornerShape(Groups.INNER_DP.dp),
    onLongClick: (() -> Unit)? = null,
) {
    val menuLabel = stringResource(R.string.list_row_actions)
    val surface = if (row.unread) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.surfaceContainerLow
    Row(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(surface)
            .combinedClickable(
                onClick = onClick,
                onLongClick = onLongClick,
                onLongClickLabel = if (onLongClick != null) menuLabel else null,
            )
            .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Avatar(row.initial, size = 44.dp, strong = row.unread)
        RowText(row, Modifier.weight(1f))
    }
}

/**
 * The letter of a sender. [strong] is the form for what wants attention (unread mail, the open
 * message): a rounded square in the primary colour. Otherwise a quiet circle.
 */
@Composable
fun Avatar(initial: String, size: Dp = 40.dp, strong: Boolean = false) {
    val colors = MaterialTheme.colorScheme
    Box(
        Modifier
            .size(size)
            .clip(if (strong) RoundedCornerShape(size * 0.375f) else CircleShape)
            .background(if (strong) colors.primary else colors.secondaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            initial,
            color = if (strong) colors.onPrimary else colors.onSecondaryContainer,
            fontWeight = FontWeight.Bold,
            fontSize = (size.value * 0.38f).sp,
        )
    }
}

@Composable
private fun RowText(row: RowContent, modifier: Modifier) {
    val weight = if (row.unread) FontWeight.Bold else FontWeight.Normal
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val time = rowTimeText(row.receivedAt)
    Column(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                row.sender,
                modifier = Modifier.weight(1f),
                fontWeight = weight,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyLarge,
            )
            Spacer(Modifier.width(8.dp))
            Text(
                time,
                color = if (row.unread) MaterialTheme.colorScheme.primary else muted,
                fontWeight = weight,
                style = MaterialTheme.typography.labelMedium,
                maxLines = 1,
            )
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                row.subject.ifEmpty { stringResource(R.string.list_no_subject) },
                modifier = Modifier.weight(1f),
                fontWeight = weight,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyMedium,
            )
            if (row.hasAttachment) {
                Spacer(Modifier.width(6.dp))
                Icon(
                    painterResource(R.drawable.ic_attachment),
                    contentDescription = stringResource(R.string.list_has_attachment),
                    modifier = Modifier.size(16.dp),
                    tint = muted,
                )
            }
            if (row.flagged) {
                Spacer(Modifier.width(4.dp))
                Icon(
                    Icons.Filled.Star,
                    contentDescription = stringResource(R.string.list_flagged),
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
            }
        }
        if (row.preview.isNotEmpty()) {
            Text(
                row.preview,
                color = muted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

@Composable
fun rowTimeText(receivedAt: Long): String {
    val yesterday = stringResource(R.string.list_yesterday)
    return remember(receivedAt, yesterday) {
        RowTime.format(receivedAt, System.currentTimeMillis(), ZoneId.systemDefault(), Locale.getDefault(), yesterday)
    }
}

/** The name of a folder: the system ones in the app's words, the others as the user named them. */
@Composable
fun folderName(mailbox: Mailbox): String = when (mailbox.role) {
    "inbox" -> stringResource(R.string.folder_inbox)
    "drafts" -> stringResource(R.string.folder_drafts)
    "sent" -> stringResource(R.string.folder_sent)
    "archive" -> stringResource(R.string.folder_archive)
    "junk" -> stringResource(R.string.folder_junk)
    "trash" -> stringResource(R.string.folder_trash)
    else -> mailbox.name
}

/** The folders to move to, as a plain list. [exclude] (the folder the mail is in) is left out. */
@Composable
fun MoveDialog(folders: List<FolderEntry>, exclude: String?, onPick: (Mailbox) -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.action_move_title)) },
        text = {
            LazyColumn(Modifier.heightIn(max = 360.dp)) {
                items(folders.filter { it.mailbox.id != exclude }, key = { it.mailbox.id }) { entry ->
                    Text(
                        folderName(entry.mailbox),
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onPick(entry.mailbox) }
                            .padding(start = (16 * entry.depth).dp, top = 12.dp, bottom = 12.dp),
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** Asks before a permanent delete (Delete in Trash). */
@Composable
fun DeleteForeverDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.delete_forever_title)) },
        text = { Text(stringResource(R.string.delete_forever_text)) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.delete_forever_confirm)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
