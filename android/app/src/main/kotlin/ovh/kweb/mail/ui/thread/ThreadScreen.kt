package ovh.kweb.mail.ui.thread

import android.app.Application
import android.content.ActivityNotFoundException
import android.content.Intent
import android.provider.DocumentsContract
import android.util.Log
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import androidx.lifecycle.ViewModelProvider.AndroidViewModelFactory.Companion.APPLICATION_KEY
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.assistant.AssistantEvent
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.assistant.AssistantSheet
import ovh.kweb.mail.ui.assistant.AssistantViewModel
import ovh.kweb.mail.ui.list.DeleteForeverDialog
import ovh.kweb.mail.ui.list.Folders
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.list.MoveDialog
import uniffi.kweb_core.AiTask
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.DraftKind

/** One conversation: its messages oldest first as cards, and what can be done with the thread. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThreadScreen(route: Route.Thread, onBack: () -> Unit, onCompose: (draftId: String) -> Unit) {
    val context = LocalContext.current
    val vm: ThreadViewModel = viewModel(
        key = "thread:${route.threadId}:${route.mailboxId}:${route.messageId}",
        factory = viewModelFactory { initializer { ThreadViewModel(this[APPLICATION_KEY] as Application, route) } },
    )
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val scope = rememberCoroutineScope()
    val activity = LocalActivity.current
    var moving by remember { mutableStateOf(false) }
    var confirmingDelete by remember { mutableStateOf(false) }
    var busyBlobId by remember { mutableStateOf<String?>(null) }

    fun say(textRes: Int) {
        scope.launch { snackbar.showSnackbar(resources.getString(textRes)) }
    }

    LaunchedEffect(vm) { vm.start() }

    // The AI assistant: one task at a time for the app; this screen shows only the session that is its own.
    val assistant: AssistantViewModel = viewModel()
    val assistantState by assistant.state.collectAsStateWithLifecycle()
    val owner = "thread:${route.threadId}"
    val offered = remember(assistantState.settings) { AssistantRules.visible(assistantState.settings) }
    LifecycleResumeEffect(assistant) {
        assistant.refreshSettings()
        onPauseOrDispose { }
    }
    // Leaving the thread for good ends its task and closes its sheet; a rotation keeps both.
    DisposableEffect(assistant, owner) { onDispose { if (activity?.isChangingConfigurations != true) assistant.release(owner) } }
    LaunchedEffect(assistant, owner) {
        assistant.events.collect { event ->
            // A draft someone else's sheet made is not for this screen.
            if (event is AssistantEvent.OpenDraft && event.owner == owner) onCompose(event.draftId)
        }
    }
    // Leaving the screen for good drops what it holds; a rotation keeps it.
    DisposableEffect(vm) { onDispose { if (activity?.isChangingConfigurations != true) vm.reset() } }
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            when (event) {
                is ThreadEvent.Message -> snackbar.showSnackbar(resources.getString(event.textRes))
                ThreadEvent.Close -> onBack()
                is ThreadEvent.OpenDraft -> onCompose(event.draftId)
            }
        }
    }

    val files = remember { BlobFiles(context.applicationContext) }

    /** The file of an attachment, downloaded if it is not there. */
    suspend fun fetch(attachment: Attachment) = withContext(Dispatchers.IO) {
        files.ensure(Background.core(context), attachment.blobId, files.attachmentFile(attachment.blobId, attachment.name))
    }

    fun openAttachment(attachment: Attachment) {
        if (busyBlobId != null) return
        busyBlobId = attachment.blobId
        scope.launch {
            try {
                val file = fetch(attachment)
                val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
                val intent = Intent(Intent.ACTION_VIEW)
                    .setDataAndType(uri, attachment.mimeType.ifBlank { "application/octet-stream" })
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                try {
                    context.startActivity(intent)
                } catch (e: ActivityNotFoundException) {
                    say(R.string.thread_no_app_for_file)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                say(R.string.thread_download_failed)
            } finally {
                busyBlobId = null
            }
        }
    }

    val saver = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("*/*")) { target ->
        val attachment = vm.pendingSave
        vm.pendingSave = null
        if (target == null) return@rememberLauncherForActivityResult
        if (attachment == null) {
            // We forgot what this was for (the app was restarted while the picker was open):
            // do not leave an empty document behind.
            scope.launch {
                withContext(Dispatchers.IO) {
                    try {
                        DocumentsContract.deleteDocument(context.contentResolver, target)
                    } catch (e: Exception) {
                        Log.w(Background.TAG, "removing the empty document failed: ${e.javaClass.simpleName}")
                    }
                }
                say(R.string.thread_save_failed)
            }
            return@rememberLauncherForActivityResult
        }
        busyBlobId = attachment.blobId
        scope.launch {
            try {
                val file = fetch(attachment)
                withContext(Dispatchers.IO) {
                    val out = context.contentResolver.openOutputStream(target) ?: throw java.io.IOException("no output stream")
                    out.use { sink -> file.inputStream().use { it.copyTo(sink) } }
                }
                say(R.string.thread_saved)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                say(R.string.thread_save_failed)
            } finally {
                busyBlobId = null
            }
        }
    }

    fun openLink(url: String) {
        val intent = if (UrlPolicy.isMail(url)) {
            Intent(Intent.ACTION_SENDTO, url.toUri())
        } else {
            Intent(Intent.ACTION_VIEW, url.toUri()).addCategory(Intent.CATEGORY_BROWSABLE)
        }
        try {
            context.startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            say(R.string.thread_no_app_for_link)
        } catch (e: SecurityException) {
            say(R.string.thread_no_app_for_link)
        }
    }

    val folders = remember(state.mailboxes) { Folders.ordered(state.mailboxes) }
    val from = state.mailboxes.firstOrNull { it.id == route.mailboxId }
    val junk = Folders.withRole(state.mailboxes, "junk")
    val flagged = state.messages.any { it.isFlagged }
    // Nothing to act on (not loaded yet, or none of the messages is in the folder it was opened from).
    val targets = remember(state.messages, state.mailboxes) { state.targets(route.mailboxId) }
    val canAct = state.loaded && targets.isNotEmpty()
    val hasMessages = state.messages.isNotEmpty()
    // Reply, Reply all and Forward of the bar act on the newest message of the thread.
    val replyTarget = remember(state.messages, state.mailboxes) { ThreadRules.replyTarget(state.messages, state.mailboxes) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                actions = {
                    if (from?.role != "archive") {
                        IconButton(onClick = vm::archive, enabled = canAct) {
                            Icon(painterResource(R.drawable.ic_archive), contentDescription = stringResource(R.string.action_archive))
                        }
                    }
                    IconButton(
                        onClick = {
                            if (ThreadRules.deletesForever(state.messages, targets, state.mailboxes)) confirmingDelete = true else vm.delete()
                        },
                        enabled = canAct,
                    ) {
                        Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.action_delete))
                    }
                    var menuOpen by remember { mutableStateOf(false) }
                    Box {
                        IconButton(onClick = { menuOpen = true }) {
                            Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.more_options))
                        }
                        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.action_mark_unread)) },
                                enabled = hasMessages,
                                onClick = { menuOpen = false; vm.markUnread() },
                            )
                            DropdownMenuItem(
                                text = { Text(stringResource(if (flagged) R.string.action_unstar else R.string.action_star)) },
                                enabled = hasMessages,
                                onClick = { menuOpen = false; vm.toggleFlag() },
                            )
                            if (AssistantRules.Kind.SUMMARY in offered) {
                                DropdownMenuItem(
                                    text = { Text(stringResource(R.string.assistant_summarize)) },
                                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                                    enabled = hasMessages,
                                    onClick = { menuOpen = false; assistant.open(owner, AssistantRules.Kind.SUMMARY, AiTask.SummarizeThread(route.threadId)) },
                                )
                            }
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.action_move)) },
                                enabled = canAct,
                                onClick = { menuOpen = false; moving = true },
                            )
                            if (junk != null && junk.id != route.mailboxId) {
                                DropdownMenuItem(
                                    text = { Text(stringResource(R.string.action_spam)) },
                                    enabled = canAct,
                                    onClick = { menuOpen = false; vm.spam() },
                                )
                            }
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
            Text(
                state.subject.ifEmpty { stringResource(R.string.list_no_subject) },
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 4.dp, bottom = 12.dp),
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Medium,
            )
            when {
                state.messages.isEmpty() && state.offline -> CenteredNote(stringResource(R.string.thread_unavailable))
                state.messages.isEmpty() -> Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator()
                }
                else -> state.messages.forEachIndexed { index, header ->
                    MessageCard(
                        header = header,
                        shape = Groups.shape(index, state.messages.size),
                        expanded = header.id in state.expanded,
                        body = state.bodies[header.id],
                        remoteAllowed = header.id in state.remoteAllowed,
                        myAddress = state.myAddress,
                        busyBlobId = busyBlobId,
                        actions = CardActions(
                            onToggle = { vm.toggle(header.id) },
                            onRetryBody = { vm.retryBody(header.id) },
                            onAllowRemote = { vm.allowRemote(header.id) },
                            onOpenLink = ::openLink,
                            onOpenAttachment = ::openAttachment,
                            onSaveAttachment = {
                                if (busyBlobId == null) {
                                    vm.pendingSave = it
                                    saver.launch(FileNames.safe(it.name))
                                }
                            },
                            onReply = { vm.startDraft(DraftKind.Reply(header.id)) },
                            onReplyAll = { vm.startDraft(DraftKind.ReplyAll(header.id)) },
                            onForward = { vm.startDraft(DraftKind.Forward(header.id)) },
                            onSummarize = if (AssistantRules.Kind.SUMMARY in offered) {
                                { assistant.open(owner, AssistantRules.Kind.SUMMARY, AiTask.SummarizeMessage(header.id)) }
                            } else {
                                null
                            },
                            onDraftReply = if (AssistantRules.Kind.DRAFT in offered) {
                                { assistant.open(owner, AssistantRules.Kind.DRAFT, AiTask.DraftReply(header.id, "")) }
                            } else {
                                null
                            },
                        ),
                    )
                }
            }
            // Room under the last card for the floating bar.
            Spacer(Modifier.height(96.dp))
        }
        ReplyBar(
            enabled = replyTarget != null,
            onReply = { replyTarget?.let { vm.startDraft(DraftKind.Reply(it.id)) } },
            onReplyAll = { replyTarget?.let { vm.startDraft(DraftKind.ReplyAll(it.id)) } },
            onForward = { replyTarget?.let { vm.startDraft(DraftKind.Forward(it.id)) } },
            modifier = Modifier.align(Alignment.BottomCenter).padding(16.dp),
        )
        }
    }

    assistantState.session?.takeIf { it.owner == owner }?.let { AssistantSheet(it, assistant) }
    if (moving) {
        MoveDialog(
            folders = folders,
            exclude = route.mailboxId,
            onPick = {
                moving = false
                vm.move(it.id)
            },
            onDismiss = { moving = false },
        )
    }
    if (confirmingDelete) {
        DeleteForeverDialog(
            onConfirm = {
                confirmingDelete = false
                vm.delete()
            },
            onDismiss = { confirmingDelete = false },
        )
    }
}

@Composable
private fun CenteredNote(text: String) {
    Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
        Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
