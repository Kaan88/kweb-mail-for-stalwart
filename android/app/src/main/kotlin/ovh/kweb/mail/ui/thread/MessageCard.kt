package ovh.kweb.mail.ui.thread

import android.util.Log
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.io.FileInputStream
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.list.Avatar
import ovh.kweb.mail.ui.list.Preview
import ovh.kweb.mail.ui.list.RowTime
import ovh.kweb.mail.ui.list.Sender
import ovh.kweb.mail.ui.list.rowTimeText
import ovh.kweb.mail.ui.mailextras.MessageChips
import ovh.kweb.mail.ui.mailextras.UnsubEntry
import uniffi.kweb_core.Address
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.MessageBody
import uniffi.kweb_core.MessageHeader

/** What a card needs to do beyond drawing. */
class CardActions(
    val onToggle: () -> Unit,
    val onRetryBody: () -> Unit,
    val onAllowRemote: () -> Unit,
    val onOpenLink: (String) -> Unit,
    val onOpenAttachment: (Attachment) -> Unit,
    val onSaveAttachment: (Attachment) -> Unit,
    val onReply: () -> Unit,
    val onReplyAll: () -> Unit,
    val onForward: () -> Unit,
    /** "Summarize" in the message's menu; null when the assistant does not offer it. */
    val onSummarize: (() -> Unit)? = null,
    /** "Draft a reply" in the message's menu; null when the assistant does not offer it. */
    val onDraftReply: (() -> Unit)? = null,
    /** "Filter mail like this" in the message's menu; null where it does not apply (a shared folder). */
    val onFilterLike: (() -> Unit)? = null,
    /** "Block sender" in the message's menu; null where it does not apply (a shared folder, or a message the user sent). */
    val onBlockSender: (() -> Unit)? = null,
    /** "Labels" in the message's menu, and what a tap on a label chip does; null where labels do not apply. */
    val onLabels: (() -> Unit)? = null,
    /** A tap on the Unsubscribe chip (it asks first). */
    val onUnsubscribe: () -> Unit = {},
    /** False for a message that may not be answered (one of a shared folder): the card has no menu. */
    val showMenu: Boolean = true,
)

/**
 * One message of the thread: a line on a quiet surface when collapsed, the whole message on a
 * raised one when expanded. [shape] is its place in the group of the thread's messages.
 */
@Composable
fun MessageCard(
    header: MessageHeader,
    shape: Shape,
    expanded: Boolean,
    body: BodyState?,
    remoteAllowed: Boolean,
    myAddress: String?,
    busyBlobId: String?,
    actions: CardActions,
    /** The owner's account id when the message is in a shared account (its pictures and files come from there); null for the user's own. */
    account: String? = null,
    /** The names of the message's labels. */
    labelNames: List<String> = emptyList(),
    /** The Unsubscribe chip; null for none. */
    unsubscribe: UnsubEntry? = null,
) {
    val sender = Sender.label(header.fromName, header.fromEmail) ?: stringResource(R.string.list_unknown_sender)
    val initial = Sender.initial(header.fromName, header.fromEmail)
    val surface = if (expanded) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.surfaceContainerLow
    Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 1.5.dp).clip(shape).background(surface)) {
        if (!expanded) {
            Row(
                Modifier.fillMaxWidth().clickable(onClick = actions.onToggle).padding(12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Avatar(initial, size = 36.dp)
                Column(Modifier.weight(1f)) {
                    Text(
                        sender,
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        Preview.oneLine(header.preview),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                Text(
                    rowTimeText(header.receivedAt),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 1,
                )
            }
        } else {
            ExpandedHeader(header, sender, initial, body, myAddress, actions, labelNames, unsubscribe)
            Column(Modifier.padding(start = 12.dp, end = 12.dp, bottom = 12.dp)) {
                BodyView(header, body, remoteAllowed, busyBlobId, actions, account)
            }
        }
    }
}

@Composable
private fun ExpandedHeader(
    header: MessageHeader,
    sender: String,
    initial: String,
    body: BodyState?,
    myAddress: String?,
    actions: CardActions,
    labelNames: List<String>,
    unsubscribe: UnsubEntry?,
) {
    var showDetails by remember(header.id) { mutableStateOf(false) }
    val loaded = (body as? BodyState.Loaded)?.body
    // The headers have the To list already; the body adds Cc. Until it is there, To has to do.
    val to = loaded?.to ?: header.to
    val cc = loaded?.cc.orEmpty()
    val meLabel = stringResource(R.string.thread_me)
    val names = remember(to, cc, myAddress, meLabel) { ThreadRules.recipientNames(to, cc, myAddress, meLabel) }
    // Short, as in the list; the full date and time are in the details a tap on the names opens.
    val yesterday = stringResource(R.string.list_yesterday)
    val time = remember(header.receivedAt, yesterday) {
        RowTime.format(header.receivedAt, System.currentTimeMillis(), ZoneId.systemDefault(), Locale.getDefault(), yesterday)
    }
    Column(Modifier.fillMaxWidth().clickable(onClick = actions.onToggle).padding(start = 12.dp, top = 12.dp, bottom = 12.dp, end = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Avatar(initial, size = 36.dp, strong = true)
            Column(Modifier.weight(1f)) {
                Text(sender, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (names != null) {
                    Text(
                        stringResource(R.string.thread_to_line, names),
                        modifier = Modifier.clickable { showDetails = !showDetails },
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
            Text(time, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
            if (actions.showMenu) MessageMenu(actions) else Spacer(Modifier.width(12.dp))
        }
        // Under the sender line, level with its text (past the avatar).
        MessageChips(
            labelNames = labelNames,
            unsubscribe = unsubscribe,
            onLabels = actions.onLabels,
            onUnsubscribe = actions.onUnsubscribe,
            modifier = Modifier.padding(start = 48.dp, top = 6.dp, end = 8.dp),
        )
        if (showDetails) {
            SelectionContainer {
                Column(Modifier.padding(top = 8.dp)) {
                    DetailLine(R.string.thread_from, listOf(Address(header.fromName, header.fromEmail)))
                    DetailLine(R.string.thread_to, to)
                    DetailLine(R.string.thread_cc, cc)
                    Text(
                        fullTime(header.receivedAt),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}

/** What can be done with this one message: answer it or pass it on. */
@Composable
private fun MessageMenu(actions: CardActions) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) {
            Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.more_options))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(R.string.action_reply)) },
                leadingIcon = { Icon(painterResource(R.drawable.ic_reply), contentDescription = null) },
                onClick = { open = false; actions.onReply() },
            )
            DropdownMenuItem(
                text = { Text(stringResource(R.string.action_reply_all)) },
                leadingIcon = { Icon(painterResource(R.drawable.ic_reply_all), contentDescription = null) },
                onClick = { open = false; actions.onReplyAll() },
            )
            DropdownMenuItem(
                text = { Text(stringResource(R.string.action_forward)) },
                leadingIcon = { Icon(painterResource(R.drawable.ic_forward), contentDescription = null) },
                onClick = { open = false; actions.onForward() },
            )
            actions.onFilterLike?.let { filter ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.filters_menu_item)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_mail), contentDescription = null) },
                    onClick = { open = false; filter() },
                )
            }
            actions.onBlockSender?.let { block ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.block_sender)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_spam), contentDescription = null) },
                    onClick = { open = false; block() },
                )
            }
            actions.onLabels?.let { labels ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.labels_menu)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_label), contentDescription = null) },
                    onClick = { open = false; labels() },
                )
            }
            actions.onSummarize?.let { summarize ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.assistant_summarize)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                    onClick = { open = false; summarize() },
                )
            }
            actions.onDraftReply?.let { draft ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.assistant_draft_reply)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                    onClick = { open = false; draft() },
                )
            }
        }
    }
}

@Composable
private fun DetailLine(labelRes: Int, addresses: List<Address>) {
    if (addresses.isEmpty()) return
    Text(
        stringResource(labelRes) + " " + addresses.joinToString(", ") { ThreadRules.fullAddress(it) },
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun BodyView(
    header: MessageHeader,
    state: BodyState?,
    remoteAllowed: Boolean,
    busyBlobId: String?,
    actions: CardActions,
    account: String?,
) {
    when (state) {
        null, BodyState.Loading -> Row(Modifier.padding(vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(12.dp))
            Text(stringResource(R.string.thread_loading_body), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        BodyState.Failed -> Column(Modifier.padding(vertical = 8.dp)) {
            Text(stringResource(R.string.thread_body_failed), color = MaterialTheme.colorScheme.onSurfaceVariant)
            TextButton(onClick = actions.onRetryBody) { Text(stringResource(R.string.retry)) }
        }
        is BodyState.Loaded -> LoadedBody(header, state.body, remoteAllowed, busyBlobId, actions, account)
    }
}

@Composable
private fun LoadedBody(
    header: MessageHeader,
    body: MessageBody,
    remoteAllowed: Boolean,
    busyBlobId: String?,
    actions: CardActions,
    account: String?,
) {
    val html = body.html
    if (html != null) {
        var blocked by remember(header.id) { mutableStateOf(false) }
        if (blocked && !remoteAllowed) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(bottom = 8.dp)
                    .clip(RoundedCornerShape(14.dp))
                    .background(MaterialTheme.colorScheme.tertiaryContainer)
                    .padding(start = 14.dp, end = 6.dp, top = 4.dp, bottom = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // A tonal bar, not a warning: nothing is wrong, the pictures are only waiting.
                Text(
                    stringResource(R.string.thread_remote_hidden),
                    modifier = Modifier.weight(1f),
                    color = MaterialTheme.colorScheme.onTertiaryContainer,
                    style = MaterialTheme.typography.labelLarge,
                )
                Button(
                    onClick = actions.onAllowRemote,
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.onTertiaryContainer,
                        contentColor = MaterialTheme.colorScheme.tertiaryContainer,
                    ),
                    contentPadding = PaddingValues(horizontal = 16.dp),
                    modifier = Modifier.height(36.dp),
                ) { Text(stringResource(R.string.thread_remote_show)) }
            }
        }
        val context = LocalContext.current
        val inline = remember(body, account) { inlineLookup(context.applicationContext, body, account) }
        MailWebView(
            html = html,
            allowRemote = remoteAllowed,
            inline = inline,
            onBlocked = { blocked = true },
            openLink = actions.onOpenLink,
            modifier = Modifier.clip(RoundedCornerShape(18.dp)),
        )
    } else {
        // On the same light inset as HTML mail, so both kinds of message look alike.
        SelectionContainer {
            Text(
                body.text.orEmpty().trimEnd(),
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(18.dp))
                    .background(MaterialTheme.colorScheme.background)
                    .padding(horizontal = 14.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
    if (body.isTruncated) {
        Text(
            stringResource(R.string.thread_truncated),
            modifier = Modifier.padding(top = 8.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
    body.attachments.filter { !it.isInline }.forEach { attachment ->
        AttachmentRow(attachment, busy = busyBlobId == attachment.blobId, actions)
    }
}

@Composable
private fun AttachmentRow(attachment: Attachment, busy: Boolean, actions: CardActions) {
    val size = remember(attachment.size) { SizeFormat.format(attachment.size.toLong(), Locale.getDefault()) }
    val kind = remember(attachment.name) { FileNames.kind(attachment.name) }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 8.dp)
            .clip(RoundedCornerShape(20.dp))
            .background(MaterialTheme.colorScheme.background),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier
                .weight(1f)
                .clickable(enabled = !busy) { actions.onOpenAttachment(attachment) }
                .padding(start = 10.dp, top = 10.dp, bottom = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // The file's kind on a small tile, as the mock-up has it; a clip when the name tells nothing.
            Box(
                Modifier.size(42.dp).clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.tertiaryContainer),
                contentAlignment = Alignment.Center,
            ) {
                val tint = MaterialTheme.colorScheme.onTertiaryContainer
                when {
                    busy -> CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = tint)
                    kind != null -> Text(kind, color = tint, fontWeight = FontWeight.Bold, style = MaterialTheme.typography.labelSmall)
                    else -> Icon(painterResource(R.drawable.ic_attachment), contentDescription = null, modifier = Modifier.size(20.dp), tint = tint)
                }
            }
            Column(Modifier.weight(1f)) {
                Text(
                    attachment.name.ifBlank { stringResource(R.string.thread_attachment) },
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(size, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            }
        }
        TextButton(onClick = { actions.onSaveAttachment(attachment) }, enabled = !busy) {
            Text(stringResource(R.string.thread_save))
        }
    }
}

/**
 * Finds the picture a `cid:` address in the HTML stands for and opens it, downloading it first
 * when it is not in the cache directory. Called on a WebView thread, so it may block. Only
 * pictures: the view must not be handed a document or a script through a Content-ID.
 */
private fun inlineLookup(context: android.content.Context, body: MessageBody, account: String?): (String) -> Pair<String, java.io.InputStream>? {
    val files = BlobFiles(context)
    return lookup@{ contentId ->
        val attachment = body.attachments.firstOrNull {
            it.contentId == contentId && it.mimeType.startsWith("image/", ignoreCase = true)
        } ?: return@lookup null
        try {
            val file = files.ensure(Background.core(context), attachment.blobId, files.inlineFile(attachment.blobId, account), account)
            attachment.mimeType to FileInputStream(file)
        } catch (e: Exception) {
            // The class only: the message of a failed download may name the blob.
            Log.w(Background.TAG, "loading an inline picture failed: ${e.javaClass.simpleName}")
            null
        }
    }
}

private fun fullTime(epochSeconds: Long): String =
    DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
        .withLocale(Locale.getDefault())
        .format(Instant.ofEpochSecond(epochSeconds).atZone(ZoneId.systemDefault()))
