package ovh.kweb.mail.ui.thread

import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.accounts.LocalAccountSession
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
import androidx.compose.material.icons.filled.Star
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
import androidx.compose.ui.graphics.Color
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
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.ui.platform.LocalResources
import ovh.kweb.mail.ui.transfer.DownloadProgress
import ovh.kweb.mail.ui.transfer.DownloadRules
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.list.Avatar
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.list.Preview
import ovh.kweb.mail.ui.list.RowTime
import ovh.kweb.mail.ui.list.Sender
import ovh.kweb.mail.ui.list.rowTimeText
import ovh.kweb.mail.ui.crypto.CryptoChip
import ovh.kweb.mail.ui.crypto.CryptoRules
import ovh.kweb.mail.ui.crypto.CryptoUi
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
    /** "Always from this sender" beside Show; null where it does not apply (a shared account never remembers pictures). */
    val onAllowRemoteAlways: (() -> Unit)? = null,
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
    /** "Reply ideas" in the message's menu; null when the assistant does not offer it. */
    val onReplyIdeas: (() -> Unit)? = null,
    /** "Find to-dos and dates" in the message's menu; null when the assistant does not offer it. */
    val onFindDates: (() -> Unit)? = null,
    /** "Explain" in the message's menu; null when the assistant does not offer it. */
    val onExplain: (() -> Unit)? = null,
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
    /** The Cancel on the attachment that is being downloaded. */
    val onCancelDownload: () -> Unit = {},
    /** A tap on the status chip of a signed or encrypted message (it opens the explanation). */
    val onCryptoChip: () -> Unit = {},
    /** A tap on the sender mark (it opens the explanation). */
    val onSenderChip: () -> Unit = {},
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
    /** How far the download of the attachment named by [busyBlobId] is; null when not known (a plain spinner). */
    download: DownloadProgress? = null,
    /** The owner's account id when the message is in a shared account (its pictures and files come from there); null for the user's own. */
    account: String? = null,
    /** The names of the message's labels. */
    labelNames: List<String> = emptyList(),
    /** The Unsubscribe chip; null for none. */
    unsubscribe: UnsubEntry? = null,
    /** The status of a signed or encrypted message; null for plain mail. */
    crypto: CryptoUi? = null,
    /** A tap on a file of an opened encrypted message. */
    onOpenCryptoFile: (uniffi.kweb_core.CryptoAttachment) -> Unit = {},
    /** A thread of one message is drawn flat on the page (subject, sender, body, files), as in the owner's sheet, not as a card. */
    flat: Boolean = false,
    /** The recipients the server refused when it took this message, which went to the others; empty for any other message. */
    refused: List<uniffi.kweb_core.RefusedRecipient> = emptyList(),
    /** What the server found out about the sender; null while not known or for mail that is not checked. */
    senderCheck: uniffi.kweb_core.SenderCheck? = null,
    /** How many messages the thread has: in a long conversation the quoted text of a message is folded (QuoteFolding). */
    messageCount: Int = 1,
) {
    val sender = Sender.label(header.fromName, header.fromEmail) ?: stringResource(R.string.list_unknown_sender)
    val initial = Sender.initial(header.fromName, header.fromEmail)
    val flatNow = flat && expanded
    val surface = when {
        flatNow -> Color.Transparent
        expanded -> MaterialTheme.colorScheme.surfaceContainerHigh
        else -> MaterialTheme.colorScheme.surfaceContainerLow
    }
    // Flat: the page's own 20dp edge (8 here + 12 inside), the same as the subject above.
    Column(Modifier.fillMaxWidth().padding(horizontal = if (flatNow) 8.dp else 12.dp, vertical = 1.5.dp).clip(shape).background(surface)) {
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
                        // Bold only while the message is unread.
                        fontWeight = if (header.isUnread) FontWeight.Bold else FontWeight.Normal,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        Preview.snippet(header.preview),
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
            ExpandedHeader(header, sender, initial, body, myAddress, actions, labelNames, unsubscribe, crypto, refused, senderCheck)
            Column(Modifier.padding(start = 12.dp, end = 12.dp, bottom = 12.dp)) {
                // Mail the core opened (decrypted, or taken out of opaque signed data) is shown from the core's copy alone;
                // remote content stays off for it when the core says so, whatever the user allowed.
                val view = (crypto as? CryptoUi.Ready)?.view
                val shown = if (body is BodyState.Loaded && view != null) BodyState.Loaded(CryptoRules.displayBody(body.body, view)) else body
                BodyView(header, shown, CryptoRules.remoteAllowed(remoteAllowed, view), busyBlobId, download, actions, account, flatNow, CryptoRules.remoteOffered(view), messageCount)
                view?.content?.attachments?.filter { !it.isInline }?.takeIf { it.isNotEmpty() }?.let { files ->
                    ovh.kweb.mail.ui.crypto.CryptoFiles(files, flatNow, onOpenCryptoFile)
                }
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
    crypto: CryptoUi?,
    refused: List<uniffi.kweb_core.RefusedRecipient>,
    senderCheck: uniffi.kweb_core.SenderCheck?,
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
    Column(Modifier.fillMaxWidth().clickable(onClick = actions.onToggle).padding(start = 12.dp, top = 12.dp, bottom = 8.dp, end = 4.dp)) {
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
            if (header.isFlagged) {
                Icon(
                    Icons.Filled.Star,
                    contentDescription = stringResource(R.string.list_flagged),
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
            }
            Text(time, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
            if (actions.showMenu) MessageMenu(actions) else Spacer(Modifier.width(12.dp))
        }
        // Under the sender line, level with its text (past the avatar): the sender check, the signature status, the labels and
        // Unsubscribe share one compact row, which wraps only when the window is too narrow for it.
        val showSender = senderCheck != null && ovh.kweb.mail.ui.sender.SenderRules.shows(senderCheck)
        MessageChips(
            labelNames = labelNames,
            unsubscribe = unsubscribe,
            onLabels = actions.onLabels,
            onUnsubscribe = actions.onUnsubscribe,
            modifier = Modifier.padding(start = 48.dp, top = 4.dp, end = 8.dp),
            leading = if (showSender || crypto != null) {
                {
                    if (showSender && senderCheck != null) {
                        ovh.kweb.mail.ui.sender.SenderChip(senderCheck, onClick = actions.onSenderChip, compact = true)
                    }
                    if (crypto != null) CryptoChip(crypto, onClick = actions.onCryptoChip, compact = true)
                }
            } else {
                null
            },
        )
        if (showSender && senderCheck != null && ovh.kweb.mail.ui.sender.SenderRules.warns(senderCheck)) {
            ovh.kweb.mail.ui.sender.SenderWarning(senderCheck, modifier = Modifier.padding(start = 48.dp, top = 4.dp, end = 8.dp))
        }
        if (refused.isNotEmpty()) {
            // The message went to the others; the server would not take these (its reply is in the details).
            Text(
                stringResource(R.string.thread_not_delivered, refused.joinToString(", ") { it.address }),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 48.dp, top = 6.dp, end = 8.dp),
            )
        }
        if (showDetails) {
            SelectionContainer {
                Column(Modifier.padding(top = 8.dp)) {
                    refused.filter { it.reply.isNotBlank() }.forEach {
                        Text(
                            stringResource(R.string.thread_not_delivered_reply, it.address, it.reply),
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
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
                    leadingIcon = { Icon(painterResource(R.drawable.ic_filter), contentDescription = null) },
                    onClick = { open = false; filter() },
                )
            }
            actions.onBlockSender?.let { block ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.block_sender)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_block), contentDescription = null) },
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
            actions.onReplyIdeas?.let { ideas ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.assistant_reply_ideas)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                    onClick = { open = false; ideas() },
                )
            }
            actions.onFindDates?.let { find ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.assistant_find_dates)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                    onClick = { open = false; find() },
                )
            }
            actions.onExplain?.let { explain ->
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.assistant_explain)) },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                    onClick = { open = false; explain() },
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
    download: DownloadProgress?,
    actions: CardActions,
    account: String?,
    flat: Boolean,
    remoteOffered: Boolean = true,
    messageCount: Int = 1,
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
        is BodyState.Loaded -> LoadedBody(header, state.body, remoteAllowed, busyBlobId, download, actions, account, flat, remoteOffered, messageCount)
    }
}

@Composable
private fun LoadedBody(
    header: MessageHeader,
    body: MessageBody,
    remoteAllowed: Boolean,
    busyBlobId: String?,
    download: DownloadProgress?,
    actions: CardActions,
    account: String?,
    flat: Boolean,
    remoteOffered: Boolean = true,
    messageCount: Int = 1,
) {
    val html = body.html
    val foldAll = QuoteFolding.foldAll(messageCount)
    val size = ovh.kweb.mail.ui.UiPrefs(LocalContext.current).textSize
    if (html != null) {
        var blocked by remember(header.id) { mutableStateOf(false) }
        if (blocked && !remoteAllowed && remoteOffered) {
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
                var menu by remember { mutableStateOf(false) }
                val always = actions.onAllowRemoteAlways
                Box {
                    Button(
                        onClick = { if (always == null) actions.onAllowRemote() else menu = true },
                        colors = ButtonDefaults.buttonColors(
                            containerColor = MaterialTheme.colorScheme.onTertiaryContainer,
                            contentColor = MaterialTheme.colorScheme.tertiaryContainer,
                        ),
                        contentPadding = PaddingValues(horizontal = 16.dp),
                        modifier = Modifier.height(36.dp),
                    ) { Text(stringResource(R.string.thread_remote_show)) }
                    if (always != null) {
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.reading_show_once)) },
                                onClick = { menu = false; actions.onAllowRemote() },
                            )
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.reading_show_always_sender)) },
                                onClick = { menu = false; always() },
                            )
                        }
                    }
                }
            }
        }
        val context = LocalContext.current
        val session = LocalAccountSession.current
        val inline = remember(body, account, session) { inlineLookup(context.applicationContext, session, body, account) }
        // Quoted text is wrapped in <details> (no script needed); the mail's own text is kept as it is.
        val quoteLabel = stringResource(R.string.thread_quote_label)
        val folded = remember(html, quoteLabel, foldAll) { QuoteFolding.foldHtml(html, quoteLabel, foldAll) }
        MailWebView(
            html = folded,
            allowRemote = remoteAllowed,
            inline = inline,
            onBlocked = { blocked = true },
            openLink = actions.onOpenLink,
            // Flat: the text sits straight on the page, on its edge; otherwise on a light inset in the card.
            modifier = if (flat) Modifier else Modifier.clip(RoundedCornerShape(18.dp)).background(MaterialTheme.colorScheme.background),
            flat = flat,
            textZoom = size.percent,
        )
    } else {
        // On the same light inset as HTML mail, so both kinds of message look alike.
        val text = body.text.orEmpty().trimEnd()
        val parts = remember(text, foldAll) { QuoteFolding.splitText(text, foldAll) }
        val textStyle = MaterialTheme.typography.bodyMedium.let { it.copy(fontSize = it.fontSize * size.factor, lineHeight = it.lineHeight * size.factor) }
        SelectionContainer {
            Column(
                if (flat) {
                    Modifier.fillMaxWidth().padding(vertical = 12.dp)
                } else {
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(18.dp))
                        .background(MaterialTheme.colorScheme.background)
                        .padding(horizontal = 14.dp, vertical = 12.dp)
                },
            ) {
                parts.forEachIndexed { index, part ->
                    if (!part.quoted) {
                        Text(part.text, style = textStyle)
                    } else {
                        var open by remember(header.id, index) { mutableStateOf(false) }
                        Text(
                            "… " + stringResource(R.string.thread_quote_label),
                            modifier = Modifier
                                .padding(vertical = 4.dp)
                                .clip(RoundedCornerShape(10.dp))
                                .clickable { open = !open }
                                .padding(horizontal = 8.dp, vertical = 2.dp),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.labelMedium,
                        )
                        if (open) Text(part.text, style = textStyle, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
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
    // The files are one group: large corners at its ends, small ones between, a small gap.
    val files = body.attachments.filter { !it.isInline }
    if (files.isNotEmpty()) {
        Column(Modifier.padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            files.forEachIndexed { index, attachment ->
                AttachmentRow(attachment, busy = busyBlobId == attachment.blobId, download, actions, Groups.shape(index, files.size), flat)
            }
        }
    }
}

@Composable
private fun AttachmentRow(attachment: Attachment, busy: Boolean, download: DownloadProgress?, actions: CardActions, shape: Shape, flat: Boolean) {
    val size = remember(attachment.size) { SizeFormat.format(attachment.size.toLong(), Locale.getDefault()) }
    val kind = remember(attachment.name) { FileNames.kind(attachment.name) }
    val resources = LocalResources.current
    val locale = remember { Locale.getDefault() }
    // While it downloads the line says how much has arrived; the bar is filled where the size is known.
    val fraction = if (busy && download != null) DownloadRules.fraction(download) else null
    val progressLine = if (busy && download != null) {
        DownloadRules.line(download, locale) { done, total -> resources.getString(R.string.download_of, done, total) }
    } else {
        null
    }
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            // Flat, the page behind is the background itself, so the tile is a raised card; in a card it is the light inset.
            .background(if (flat) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.background),
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
                    busy && fraction != null -> CircularProgressIndicator(progress = { fraction }, modifier = Modifier.size(20.dp), strokeWidth = 2.dp, color = tint, trackColor = tint.copy(alpha = 0.2f))
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
                Text(progressLine ?: size, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
                if (busy) {
                    if (fraction != null) {
                        LinearProgressIndicator(progress = { fraction }, modifier = Modifier.fillMaxWidth().padding(top = 6.dp), drawStopIndicator = {})
                    } else {
                        LinearProgressIndicator(modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
                    }
                }
            }
        }
        if (busy) {
            TextButton(onClick = actions.onCancelDownload) { Text(stringResource(R.string.cancel)) }
        } else {
            TextButton(onClick = { actions.onSaveAttachment(attachment) }) {
                Text(stringResource(R.string.thread_save))
            }
        }
    }
}

/**
 * Finds the picture a `cid:` address in the HTML stands for and opens it, downloading it first
 * when it is not in the cache directory. Called on a WebView thread, so it may block. Only
 * pictures: the view must not be handed a document or a script through a Content-ID.
 */
private fun inlineLookup(context: android.content.Context, session: AccountSession, body: MessageBody, account: String?): (String) -> Pair<String, java.io.InputStream>? {
    val files = BlobFiles(context, session.record.id)
    return lookup@{ contentId ->
        val attachment = body.attachments.firstOrNull {
            it.contentId == contentId && it.mimeType.startsWith("image/", ignoreCase = true)
        } ?: return@lookup null
        try {
            val file = files.ensure(session.core, attachment.blobId, files.inlineFile(attachment.blobId, account), account)
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
