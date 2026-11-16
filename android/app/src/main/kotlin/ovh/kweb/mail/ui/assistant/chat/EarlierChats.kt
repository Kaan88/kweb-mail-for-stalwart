package ovh.kweb.mail.ui.assistant.chat

import android.text.format.DateUtils
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import uniffi.kweb_core.AiChatSummary

/**
 * The chats kept on this phone for the account, the most recently used first. A tap opens one and the next message
 * continues it; the bin takes one away. [chats] is null while the list is being read.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EarlierChatsSheet(
    chats: List<AiChatSummary>?,
    onOpen: (String) -> Unit,
    onDelete: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.navigationBarsPadding().padding(bottom = 16.dp)) {
            Text(
                stringResource(R.string.chat_earlier),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
            )
            when {
                chats == null -> CircularProgressIndicator(Modifier.padding(24.dp))
                chats.isEmpty() -> Text(
                    stringResource(R.string.chat_earlier_empty),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 16.dp),
                )
                else -> LazyColumn(Modifier.heightIn(max = 480.dp)) {
                    items(chats, key = { it.id }) { chat ->
                        Row(
                            Modifier.fillMaxWidth().clickable { onOpen(chat.id) }.padding(start = 24.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(Modifier.weight(1f)) {
                                Text(chat.title, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
                                val when_ = DateUtils.getRelativeTimeSpanString(chat.updatedAt * 1000L, System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS).toString()
                                Text(
                                    stringResource(R.string.chat_earlier_row, when_, chat.model),
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    style = MaterialTheme.typography.bodyMedium,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                chat.aboutSubject?.takeIf { it.isNotBlank() }?.let {
                                    Text(
                                        stringResource(R.string.chat_earlier_about, it),
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        style = MaterialTheme.typography.bodySmall,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                            }
                            IconButton(onClick = { onDelete(chat.id) }) {
                                Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.chat_earlier_delete))
                            }
                        }
                    }
                }
            }
        }
    }
}
