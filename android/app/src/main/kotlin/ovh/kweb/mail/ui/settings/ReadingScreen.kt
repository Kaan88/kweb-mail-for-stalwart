package ovh.kweb.mail.ui.settings

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.ReadingRules
import ovh.kweb.mail.ui.UiPrefs
import ovh.kweb.mail.ui.accounts.LocalAccountSession
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch
import uniffi.kweb_core.RemotePictures

/** One choice of a list of choices: the value and the text the dialog shows for it. */
internal class Option<T>(val value: T, @StringRes val label: Int)

private val LINE_LABELS = intArrayOf(R.string.reading_preview_none, R.string.reading_lines_1, R.string.reading_lines_2, R.string.reading_lines_3)

/**
 * "Reading": pictures (the setting and the senders allowed, kept in the core per account), and how the list and a message are
 * shown (kept in the app's own preferences, the same for every account).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReadingScreen(onBack: () -> Unit, onOpenSenders: () -> Unit) {
    val prefs = UiPrefs(LocalContext.current)
    val session = LocalAccountSession.current
    val scope = rememberCoroutineScope()
    var pictures by remember { mutableStateOf(RemotePictures.OFF) }
    var senders by remember { mutableStateOf<List<String>?>(null) }
    var conversations by remember { mutableStateOf(prefs.conversationView) }
    var markRead by remember { mutableStateOf(prefs.markRead) }
    var after by remember { mutableStateOf(prefs.afterAction) }
    var size by remember { mutableStateOf(prefs.textSize) }
    var lines by remember { mutableStateOf(prefs.previewLines) }
    var asking by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(session) {
        withContext(Dispatchers.IO) {
            runCatching { session.core.remotePictures() to session.core.remoteAllowedSenders() }.getOrNull()
        }?.let { (mode, list) ->
            pictures = mode
            senders = list
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.reading_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            SectionTitle(stringResource(R.string.reading_pictures))
            Group {
                SettingsRow(
                    Groups.shape(0, 2),
                    stringResource(R.string.reading_show_pictures),
                    stringResource(if (pictures == RemotePictures.ALWAYS) R.string.reading_pictures_always else R.string.reading_pictures_never),
                    onClick = { asking = "pictures" },
                )
                SettingsRow(
                    Groups.shape(1, 2),
                    stringResource(R.string.reading_senders),
                    senders?.size?.let { if (it == 0) stringResource(R.string.reading_senders_none) else it.toString() },
                    onClick = onOpenSenders,
                )
            }
            Text(
                stringResource(R.string.reading_pictures_note),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            SectionTitle(stringResource(R.string.reading_list))
            Group {
                SettingsRow(
                    Groups.shape(0, 2),
                    stringResource(R.string.reading_conversations),
                    stringResource(if (conversations) R.string.reading_conversations_on else R.string.reading_conversations_off),
                    trailing = {
                        KwebSwitch(checked = conversations, onCheckedChange = { conversations = it; prefs.conversationView = it })
                    },
                    onClick = { conversations = !conversations; prefs.conversationView = conversations },
                )
                SettingsRow(
                    Groups.shape(1, 2),
                    stringResource(R.string.reading_preview_lines),
                    if (lines == 0) stringResource(R.string.reading_preview_none) else lines.toString(),
                    onClick = { asking = "lines" },
                )
            }

            SectionTitle(stringResource(R.string.reading_message))
            Group {
                SettingsRow(Groups.shape(0, 3), stringResource(R.string.reading_text_size), stringResource(size.label), onClick = { asking = "size" })
                SettingsRow(Groups.shape(1, 3), stringResource(R.string.reading_mark_read), stringResource(markRead.label), onClick = { asking = "mark" })
                SettingsRow(Groups.shape(2, 3), stringResource(R.string.reading_after), stringResource(after.label), onClick = { asking = "after" })
            }
        }
    }

    when (asking) {
        "pictures" -> ChoiceDialog(
            R.string.reading_show_pictures,
            listOf(Option(RemotePictures.OFF, R.string.reading_pictures_never), Option(RemotePictures.ALWAYS, R.string.reading_pictures_always)),
            pictures,
            onPick = {
                pictures = it
                scope.launch { withContext(Dispatchers.IO) { runCatching { session.core.setRemotePictures(it) } } }
            },
            onDismiss = { asking = null },
        )
        "lines" -> ChoiceDialog(
            R.string.reading_preview_lines,
            (0..ReadingRules.MAX_PREVIEW_LINES).map { Option(it, LINE_LABELS[it]) },
            lines,
            onPick = { lines = it; prefs.previewLines = it },
            onDismiss = { asking = null },
        )
        "size" -> ChoiceDialog(
            R.string.reading_text_size,
            ReadingRules.TextSize.entries.map { Option(it, it.label) },
            size,
            onPick = { size = it; prefs.textSize = it },
            onDismiss = { asking = null },
        )
        "mark" -> ChoiceDialog(
            R.string.reading_mark_read,
            ReadingRules.MarkRead.entries.map { Option(it, it.label) },
            markRead,
            onPick = { markRead = it; prefs.markRead = it },
            onDismiss = { asking = null },
        )
        "after" -> ChoiceDialog(
            R.string.reading_after,
            ReadingRules.AfterAction.entries.map { Option(it, it.label) },
            after,
            onPick = { after = it; prefs.afterAction = it },
            onDismiss = { asking = null },
        )
    }
}

@Composable
internal fun <T> ChoiceDialog(
    @StringRes title: Int,
    options: List<Option<T>>,
    selected: T,
    onPick: (T) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(title)) },
        text = {
            Column {
                options.forEach { option ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .selectable(selected = option.value == selected, role = Role.RadioButton, onClick = { onPick(option.value); onDismiss() })
                            .heightIn(min = 48.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = option.value == selected, onClick = null)
                        Text(stringResource(option.label), modifier = Modifier.padding(start = 12.dp), style = MaterialTheme.typography.bodyLarge)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** The senders whose pictures show with no tap, one row each with a Remove, in the core's order (a to z). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteSendersScreen(onBack: () -> Unit) {
    val session = LocalAccountSession.current
    val scope = rememberCoroutineScope()
    var senders by remember { mutableStateOf<List<String>?>(null) }
    suspend fun load() {
        senders = withContext(Dispatchers.IO) { runCatching { session.core.remoteAllowedSenders() }.getOrNull() } ?: emptyList()
    }
    LaunchedEffect(session) { load() }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.reading_senders)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            val list = senders
            when {
                list == null -> Unit
                list.isEmpty() -> Text(
                    stringResource(R.string.reading_senders_empty),
                    modifier = Modifier.fillMaxWidth().padding(32.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
                else -> Group(Modifier.padding(top = 8.dp)) {
                    list.forEachIndexed { index, sender ->
                        SettingsRow(
                            Groups.shape(index, list.size),
                            sender,
                            trailing = {
                                IconButton(onClick = {
                                    scope.launch {
                                        withContext(Dispatchers.IO) { runCatching { session.core.forgetRemoteSender(sender) } }
                                        load()
                                    }
                                }) {
                                    Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.reading_sender_remove, sender))
                                }
                            },
                        )
                    }
                }
            }
        }
    }
}
