package ovh.kweb.mail.ui.mailextras

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Preview
import ovh.kweb.mail.ui.list.RowContent
import ovh.kweb.mail.ui.list.Sender
import uniffi.kweb_core.MessageHeader

/** A single message as a row (a search hit, a message with a label), with its label names and the search hit marked when known. */
@Composable
fun messageRowContent(m: MessageHeader, labelNames: List<String> = emptyList(), highlighted: Highlighted? = null): RowContent = RowContent(
    sender = Sender.label(m.fromName, m.fromEmail) ?: stringResource(R.string.list_unknown_sender),
    initial = Sender.initial(m.fromName, m.fromEmail),
    subject = m.subject.trim(),
    preview = Preview.oneLine(m.preview),
    receivedAt = m.receivedAt,
    unread = m.isUnread,
    flagged = m.isFlagged,
    hasAttachment = m.hasAttachment,
    labels = labelNames,
    subjectHighlight = highlighted?.subject,
    previewHighlight = highlighted?.preview,
)
