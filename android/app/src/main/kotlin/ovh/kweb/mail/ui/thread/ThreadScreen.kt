package ovh.kweb.mail.ui.thread

import ovh.kweb.mail.ui.accounts.LocalAccountSession
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
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.TextButton
import androidx.compose.runtime.saveable.rememberSaveable
import ovh.kweb.mail.ui.folders.SpamRules
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
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.LocalDate
import java.time.ZoneId
import java.io.File
import kotlinx.coroutines.CancellationException
import ovh.kweb.mail.ui.transfer.ActiveTransfer
import ovh.kweb.mail.ui.transfer.DownloadFailure
import ovh.kweb.mail.ui.transfer.DownloadRules
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
import ovh.kweb.mail.ui.assistant.chat.ChatRules
import ovh.kweb.mail.ui.calendar.EventPrefill
import ovh.kweb.mail.ui.list.DeleteForeverDialog
import ovh.kweb.mail.ui.list.Folders
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.list.MoveDialog
import ovh.kweb.mail.ui.list.Sender
import ovh.kweb.mail.ui.mailextras.LabelRules
import ovh.kweb.mail.ui.mailextras.LabelSheet
import ovh.kweb.mail.ui.mailextras.NewLabelDialog
import ovh.kweb.mail.ui.mailextras.UnsubEntry
import ovh.kweb.mail.ui.mailextras.UnsubscribeDialog
import ovh.kweb.mail.ui.mailextras.UnsubscribeRules
import uniffi.kweb_core.AiTask
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.DraftKind

/** One conversation: its messages oldest first as cards, and what can be done with the thread. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThreadScreen(
    /** The conversation's own view model (the pager keeps one per page). */
    vm: ThreadViewModel,
    route: Route.Thread,
    /** This is the page the user is on. A neighbour that is only loaded does not mark mail read, close the screen or say anything. */
    active: Boolean,
    onBack: () -> Unit,
    /** The thread was archived, deleted, moved or is gone: where to go next (the pager may step to a neighbour). */
    onClosed: () -> Unit = onBack,
    onCompose: (draftId: String) -> Unit,
    onFilterLike: (messageId: String) -> Unit = {},
    onOpenFilters: () -> Unit = {},
    /** Opens the form for a new calendar entry that starts with something the assistant found here; the user saves it himself. */
    onAddToCalendar: (EventPrefill) -> Unit = {},
    /** Opens the assistant's chat about this conversation (offered when the chat is on; the chat asks for the rest). */
    onAskAssistant: (threadId: String) -> Unit = {},
    /** What TalkBack offers on the subject to go to the conversation before or after this one (the pager has no buttons for it). */
    stepActions: List<CustomAccessibilityAction> = emptyList(),
) {
    val context = LocalContext.current
    val accountSession = LocalAccountSession.current
    val latestActive by rememberUpdatedState(active)
    LaunchedEffect(vm, active) { vm.setActive(active) }
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val scope = rememberCoroutineScope()
    val activity = LocalActivity.current
    var moving by remember { mutableStateOf(false) }
    var confirmingDelete by remember { mutableStateOf(false) }
    // The message whose sender is asked about; saved, so a rotation keeps the question.
    var blockingId by rememberSaveable { mutableStateOf<String?>(null) }
    // The attachment being downloaded (one at a time, kept by the view model so that leaving the screen can end it).
    val activeDownload by vm.transfer.state.collectAsStateWithLifecycle()
    val busyBlobId = activeDownload?.key
    // Saved, so a rotation keeps the sheet and the questions: the message whose labels are edited ("*": the whole thread),
    // whether the new-label dialog is open, and the message whose unsubscribing is asked about.
    var labelsTarget by rememberSaveable { mutableStateOf<String?>(null) }
    var newLabel by rememberSaveable { mutableStateOf(false) }
    var creatingLabel by remember { mutableStateOf(false) }
    var unsubscribeId by rememberSaveable { mutableStateOf<String?>(null) }
    val labelsState by vm.labels.state.collectAsStateWithLifecycle()
    val unsubscribes by vm.unsubscribe.state.collectAsStateWithLifecycle()
    val cryptos by vm.crypto.state.collectAsStateWithLifecycle()
    var cryptoId by rememberSaveable { mutableStateOf<String?>(null) }
    // The passphrase prompt: only the page the user is on asks, once per message until the user asks again from the chip.
    var passphraseId by rememberSaveable { mutableStateOf<String?>(null) }
    var skipped by remember { mutableStateOf(setOf<String>()) }
    LaunchedEffect(cryptos, active) {
        if (passphraseId == null) {
            passphraseId = cryptos.entries.firstOrNull { (id, ui) ->
                id !in skipped && ui is ovh.kweb.mail.ui.crypto.CryptoUi.Ready && ovh.kweb.mail.ui.crypto.CryptoRules.mayPrompt(ui.view, active)
            }?.key
        }
    }
    val cryptoSession = ovh.kweb.mail.ui.accounts.LocalAccountSession.current
    // The files of an opened encrypted message are written to a folder of their own in the cache while they are open,
    // and the folder is emptied when the screen is left.
    androidx.compose.runtime.DisposableEffect(vm) {
        onDispose { ovh.kweb.mail.ui.crypto.CryptoTemp.clear(context, cryptoSession.record.id) }
    }

    fun say(textRes: Int) {
        scope.launch { snackbar.showSnackbar(resources.getString(textRes)) }
    }

    /** Why a download failed, in the core's words; "you may be offline" only when the server could not be reached. */
    fun sayDownloadFailed(error: Throwable) {
        val text = when (val failure = DownloadRules.describe(error)) {
            DownloadFailure.Offline -> resources.getString(R.string.thread_download_failed)
            is DownloadFailure.Words -> resources.getString(R.string.thread_download_failed_why, failure.text)
            DownloadFailure.SignIn -> resources.getString(R.string.thread_download_failed_sign_in)
            DownloadFailure.Unknown -> resources.getString(R.string.thread_download_failed_unknown)
        }
        scope.launch { snackbar.showSnackbar(text) }
    }

    LaunchedEffect(vm) { vm.start() }

    // The AI assistant: one task at a time for the app; this screen shows only the session that is its own.
    val assistant: AssistantViewModel = viewModel()
    val assistantState by assistant.state.collectAsStateWithLifecycle()
    val owner = "thread:${route.threadId}"
    val offered = remember(assistantState.settings) { AssistantRules.visible(assistantState.settings) }
    LifecycleResumeEffect(assistant, active) {
        if (active) assistant.refreshSettings()
        onPauseOrDispose { }
    }
    // Leaving the thread for good ends its task and closes its sheet; a rotation keeps both.
    DisposableEffect(assistant, owner) { onDispose { if (activity?.isChangingConfigurations != true) assistant.release(owner) } }
    LaunchedEffect(assistant, owner) {
        assistant.events.collect { event ->
            // A draft or a calendar form someone else's sheet asked for is not for this screen.
            when (event) {
                is AssistantEvent.OpenDraft -> if (event.owner == owner) onCompose(event.draftId)
                is AssistantEvent.OpenEvent -> if (event.owner == owner) onAddToCalendar(event.prefill)
            }
        }
    }
    // Leaving the screen for good drops what it holds; a rotation keeps it.
    DisposableEffect(vm) { onDispose { if (activity?.isChangingConfigurations != true) vm.reset() } }
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            // A page that is only next to the shown one says nothing and does not leave.
            if (!latestActive) return@collect
            when (event) {
                is ThreadEvent.Message -> snackbar.showSnackbar(resources.getString(event.textRes))
                is ThreadEvent.Text -> snackbar.showSnackbar(event.message.resolve(resources))
                is ThreadEvent.Blocked -> snackbar.showSnackbar(resources.getString(R.string.block_done, event.address))
                is ThreadEvent.BlockRefused -> {
                    // The core's words as they are, with the way to the place where the user decides.
                    val result = snackbar.showSnackbar(
                        event.text,
                        actionLabel = resources.getString(R.string.block_open_filters),
                        duration = SnackbarDuration.Long,
                    )
                    if (result == SnackbarResult.ActionPerformed) onOpenFilters()
                }
                ThreadEvent.Close -> onClosed()
                is ThreadEvent.OpenDraft -> onCompose(event.draftId)
            }
        }
    }

    val files = remember { BlobFiles(context.applicationContext, accountSession.record.id) }

    /** The file of an attachment, downloaded if it is not there; null when the user cancelled the download. */
    suspend fun fetch(attachment: Attachment, run: ActiveTransfer.Run): File? = withContext(Dispatchers.IO) {
        files.ensureTracked(
            accountSession.core,
            attachment.blobId,
            files.attachmentFile(attachment.blobId, attachment.name),
            run.stop,
        ) { done, total -> vm.transfer.report(run, done, total) }
    }

    fun openAttachment(attachment: Attachment) {
        // A tap while a download is under way (this file or another) starts nothing.
        val run = vm.transfer.begin(attachment.blobId, DownloadRules.expected(attachment.size.toLong())) ?: return
        scope.launch {
            try {
                val file = fetch(attachment, run) ?: return@launch
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
                sayDownloadFailed(e)
            } finally {
                vm.transfer.finish(run)
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
        val run = vm.transfer.begin(attachment.blobId, DownloadRules.expected(attachment.size.toLong()))
        if (run == null) {
            // Another download is under way: the empty document the picker made must not stay.
            scope.launch { withContext(Dispatchers.IO) { runCatching { DocumentsContract.deleteDocument(context.contentResolver, target) } } }
            return@rememberLauncherForActivityResult
        }
        scope.launch {
            try {
                val file = try {
                    fetch(attachment, run)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    withContext(Dispatchers.IO) { runCatching { DocumentsContract.deleteDocument(context.contentResolver, target) } }
                    sayDownloadFailed(e)
                    return@launch
                }
                if (file == null) {
                    // Cancelled: the empty document the picker made must not stay.
                    withContext(Dispatchers.IO) { runCatching { DocumentsContract.deleteDocument(context.contentResolver, target) } }
                    return@launch
                }
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
                vm.transfer.finish(run)
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
                                leadingIcon = { Icon(painterResource(R.drawable.ic_mail), contentDescription = null) },
                                enabled = hasMessages,
                                onClick = { menuOpen = false; vm.markUnread() },
                            )
                            DropdownMenuItem(
                                text = { Text(stringResource(if (flagged) R.string.action_unstar else R.string.action_star)) },
                                leadingIcon = { Icon(Icons.Filled.Star, contentDescription = null) },
                                enabled = hasMessages,
                                onClick = { menuOpen = false; vm.toggleFlag() },
                            )
                            if (ChatRules.entryShown(assistantState.settings)) {
                                DropdownMenuItem(
                                    text = { Text(stringResource(R.string.assistant_ask_about)) },
                                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                                    enabled = hasMessages,
                                    onClick = { menuOpen = false; onAskAssistant(route.threadId) },
                                )
                            }
                            if (AssistantRules.Kind.SUMMARY in offered) {
                                DropdownMenuItem(
                                    text = { Text(stringResource(R.string.assistant_summarize)) },
                                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                                    enabled = hasMessages,
                                    onClick = { menuOpen = false; assistant.open(owner, AssistantRules.Kind.SUMMARY, AiTask.SummarizeThread(route.threadId)) },
                                )
                            }
                            if (AssistantRules.Kind.EXTRACT in offered) {
                                DropdownMenuItem(
                                    text = { Text(stringResource(R.string.assistant_find_dates)) },
                                    leadingIcon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                                    enabled = hasMessages,
                                    onClick = {
                                        menuOpen = false
                                        // The phone's date and zone are part of what is sent, so "tomorrow" in a message can be read.
                                        assistant.open(owner, AssistantRules.Kind.EXTRACT, AiTask.ExtractThread(route.threadId, LocalDate.now().toString(), ZoneId.systemDefault().id))
                                    },
                                )
                            }
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.labels_menu)) },
                                leadingIcon = { Icon(painterResource(R.drawable.ic_label), contentDescription = null) },
                                enabled = hasMessages,
                                onClick = { menuOpen = false; labelsTarget = LABELS_THREAD },
                            )
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.action_move)) },
                                leadingIcon = { Icon(painterResource(R.drawable.ic_folder), contentDescription = null) },
                                enabled = canAct,
                                onClick = { menuOpen = false; moving = true },
                            )
                            if (SpamRules.threadOffered(from, junk != null)) {
                                DropdownMenuItem(
                                    text = {
                                        val notSpam = SpamRules.threadIsNotSpam(state.messages, targets, route.mailboxId, state.mailboxes)
                                        Text(stringResource(if (notSpam) R.string.spam_not else R.string.spam_report))
                                    },
                                    leadingIcon = { Icon(painterResource(R.drawable.ic_spam), contentDescription = null) },
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
                modifier = Modifier
                    .padding(start = 20.dp, end = 20.dp, top = 4.dp, bottom = 12.dp)
                    .semantics { customActions = stepActions },
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
                        flat = state.messages.size == 1,
                        body = state.bodies[header.id],
                        remoteAllowed = header.id in state.remoteAllowed,
                        myAddress = state.myAddress,
                        busyBlobId = busyBlobId,
                        download = activeDownload?.progress,
                        labelNames = LabelRules.namesOf(labelsState.byMessage[header.id], labelsState.labels),
                        unsubscribe = unsubscribes[header.id],
                        crypto = cryptos[header.id],
                        onOpenCryptoFile = { file ->
                            scope.launch {
                                val opened = ovh.kweb.mail.ui.crypto.CryptoTemp.open(context, cryptoSession, header.id, file)
                                if (opened == null) say(R.string.crypto_attachment_failed) else context.startActivity(opened)
                            }
                        },
                        actions = CardActions(
                            onCryptoChip = { cryptoId = header.id },
                            onLabels = { labelsTarget = header.id },
                            onUnsubscribe = { unsubscribeId = header.id },
                            onToggle = { vm.toggle(header.id) },
                            onRetryBody = { vm.retryBody(header.id) },
                            onAllowRemote = { vm.allowRemote(header.id) },
                            onAllowRemoteAlways = { vm.allowRemoteAlways(header.id) },
                            onOpenLink = ::openLink,
                            onOpenAttachment = ::openAttachment,
                            onSaveAttachment = {
                                if (busyBlobId == null) {
                                    vm.pendingSave = it
                                    saver.launch(FileNames.safe(it.name))
                                }
                            },
                            onCancelDownload = vm.transfer::cancel,
                            onReply = { vm.startDraft(DraftKind.Reply(header.id)) },
                            onReplyAll = { vm.startDraft(DraftKind.ReplyAll(header.id)) },
                            onForward = { vm.startDraft(DraftKind.Forward(header.id)) },
                            onFilterLike = { onFilterLike(header.id) },
                            onBlockSender = if (ThreadRules.canBlockSender(header, state.myAddress)) {
                                { blockingId = header.id }
                            } else {
                                null
                            },
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
                            onReplyIdeas = if (AssistantRules.Kind.IDEAS in offered) {
                                { assistant.open(owner, AssistantRules.Kind.IDEAS, AiTask.ReplyIdeas(header.id)) }
                            } else {
                                null
                            },
                            onFindDates = if (AssistantRules.Kind.EXTRACT in offered) {
                                { assistant.open(owner, AssistantRules.Kind.EXTRACT, AiTask.Extract(header.id, LocalDate.now().toString(), ZoneId.systemDefault().id)) }
                            } else {
                                null
                            },
                            onExplain = if (AssistantRules.Kind.EXPLAIN in offered) {
                                { assistant.open(owner, AssistantRules.Kind.EXPLAIN, AiTask.Explain(header.id, "")) }
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
    labelsTarget?.let { target ->
        val ids = if (target == LABELS_THREAD) state.messages.map { it.id } else listOf(target).filter { id -> state.messages.any { it.id == id } }
        if (ids.isEmpty()) {
            labelsTarget = null
        } else {
            // Opening the sheet asks again, so a read that failed or has not come yet is not the last word.
            LaunchedEffect(target) {
                vm.labels.refreshLabels()
                vm.labels.load(ids)
            }
            LabelSheet(
                labels = labelsState.labels,
                messageIds = ids,
                byMessage = labelsState.byMessage,
                ready = labelsState.labelsRead && LabelRules.known(ids, labelsState.byMessage),
                error = if (newLabel) null else labelsState.error?.resolve(resources),
                onToggle = { labelId, on -> vm.labels.set(ids, labelId, on) },
                onNewLabel = { newLabel = true },
                onDismiss = { labelsTarget = null; newLabel = false; vm.labels.clearError() },
            )
            if (newLabel) {
                NewLabelDialog(
                    onCreate = { name ->
                        creatingLabel = true
                        vm.labels.create(name, ids) { made ->
                            creatingLabel = false
                            if (made) newLabel = false
                        }
                    },
                    onDismiss = { newLabel = false; vm.labels.clearError() },
                    error = labelsState.error?.resolve(resources),
                    busy = creatingLabel,
                    onEdit = vm.labels::clearError,
                )
            }
        }
    }
    passphraseId?.let { id ->
        val view = (cryptos[id] as? ovh.kweb.mail.ui.crypto.CryptoUi.Ready)?.view
        if (view == null || !ovh.kweb.mail.ui.crypto.CryptoRules.needsPassphrase(view)) {
            passphraseId = null
        } else {
            ovh.kweb.mail.ui.crypto.PassphraseDialog(
                keyName = ovh.kweb.mail.ui.crypto.CryptoRules.keyName(view),
                wrong = view.encryption == uniffi.kweb_core.EncryptionState.WRONG_PASSPHRASE,
                onSubmit = { pass, keep -> passphraseId = null; vm.crypto.open(id, pass, keep) },
                onDismiss = { skipped = skipped + id; passphraseId = null },
            )
        }
    }
    cryptoId?.let { id ->
        val ui = cryptos[id]
        if (ui == null) {
            cryptoId = null
        } else {
            ovh.kweb.mail.ui.crypto.CryptoDialog(
                ui = ui,
                onDismiss = { cryptoId = null },
                onRetry = { vm.crypto.recheck(id) },
                onTakeOffer = { offer -> vm.crypto.takeOffer(id, offer) { say(R.string.crypto_offer_failed) } },
                onPassphrase = { cryptoId = null; skipped = skipped - id; passphraseId = id },
            )
        }
    }
    unsubscribeId?.let { id ->
        val header = state.messages.firstOrNull { it.id == id }
        val entry = unsubscribes[id] as? UnsubEntry.Available
        if (header == null || entry == null) {
            unsubscribeId = null
        } else {
            UnsubscribeDialog(
                list = Sender.label(header.fromName, header.fromEmail) ?: stringResource(R.string.list_unknown_sender),
                way = entry.way,
                onConfirm = {
                    unsubscribeId = null
                    if (entry.way == UnsubscribeRules.Way.BROWSER) entry.webUrl?.let(::openLink) else vm.unsubscribe.confirm(id)
                },
                onDismiss = { unsubscribeId = null },
            )
        }
    }
    blockingId?.let { id ->
        val address = state.messages.firstOrNull { it.id == id }?.fromEmail
        if (address == null) {
            blockingId = null
        } else {
            AlertDialog(
                onDismissRequest = { blockingId = null },
                title = { Text(stringResource(R.string.block_title, address)) },
                text = { Text(stringResource(R.string.block_text, address)) },
                confirmButton = {
                    TextButton(onClick = { blockingId = null; vm.blockSender(id) }) {
                        Text(stringResource(R.string.block_confirm), color = MaterialTheme.colorScheme.error)
                    }
                },
                dismissButton = { TextButton(onClick = { blockingId = null }) { Text(stringResource(R.string.cancel)) } },
            )
        }
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

/** The labels sheet is for the whole thread, not for one message. */
private const val LABELS_THREAD = "*"

@Composable
private fun CenteredNote(text: String) {
    Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
        Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
