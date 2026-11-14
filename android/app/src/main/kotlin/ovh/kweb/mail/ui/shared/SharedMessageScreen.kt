package ovh.kweb.mail.ui.shared

import ovh.kweb.mail.ui.accounts.LocalAccountSession
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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
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
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.thread.BlobFiles
import ovh.kweb.mail.ui.thread.BodyState
import ovh.kweb.mail.ui.thread.CardActions
import ovh.kweb.mail.ui.thread.FileNames
import ovh.kweb.mail.ui.thread.MessageCard
import ovh.kweb.mail.ui.thread.UrlPolicy
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.SharedAction

/**
 * A message of a shared folder, drawn with the card and the locked-down WebView of the thread screen: the same
 * blocking of remote pictures, the same handling of links, attachments opened and saved the same way (but
 * downloaded from the owner's account). Opening it does not mark it read. What may be done to it (mark, flag, move,
 * delete) is offered only where the folder's rights allow it; there is no archive. Reply, reply to all and forward are
 * always offered: the draft is the user's own and goes from the user's own address ([onCompose] opens it).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedMessageScreen(route: Route.SharedMail, onBack: () -> Unit, onCompose: (draftId: String) -> Unit) {
    val context = LocalContext.current
    val app = context.applicationContext as Application
    val accountSession = LocalAccountSession.current
    val vm: SharedMessageViewModel = viewModel(
        factory = viewModelFactory { initializer { SharedMessageViewModel(app, accountSession, route.account, route.mailboxId, route.messageId) } },
    )
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val scope = rememberCoroutineScope()
    var confirmingDelete by rememberSaveable { mutableStateOf(false) }
    var moving by rememberSaveable { mutableStateOf(false) }
    var busyBlobId by remember { mutableStateOf<String?>(null) }

    fun say(textRes: Int) {
        scope.launch {
            snackbar.currentSnackbarData?.dismiss()
            snackbar.showSnackbar(resources.getString(textRes))
        }
    }

    // A message that is gone (moved or deleted by someone) or a share that is gone ends here; the list reads itself again.
    CollectSharedEvents(c.events, snackbar, onGone = onBack, onDone = onBack)
    // The user's own reply or forward: the ordinary compose screen opens with that draft.
    LaunchedEffect(vm) { vm.drafts.draftStarted.collect { onCompose(it) } }
    LaunchedEffect(vm) {
        vm.drafts.problems.collect {
            snackbar.currentSnackbarData?.dismiss()
            snackbar.showSnackbar(it.resolve(resources))
        }
    }
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }
    val activity = LocalActivity.current
    DisposableEffect(vm) { onDispose { if (activity?.isChangingConfigurations != true) c.reset() } }

    val files = remember { BlobFiles(context.applicationContext, accountSession.record.id) }

    /** The file of an attachment, downloaded from the owner's account if it is not there. */
    suspend fun fetch(attachment: Attachment) = withContext(Dispatchers.IO) {
        files.ensure(
            accountSession.core,
            attachment.blobId,
            files.attachmentFile(attachment.blobId, attachment.name, route.account),
            route.account,
        )
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
            // We forgot what this was for (the app was restarted while the picker was open): leave no empty document behind.
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

    val shared = state.message
    val canMark = state.allows(SharedActionKind.MARK)
    val canMove = state.allows(SharedActionKind.MOVE)
    val canDelete = state.allows(SharedActionKind.DELETE)
    val starting by vm.drafts.busy.collectAsStateWithLifecycle()
    val idle = shared != null && !state.busy

    Scaffold(
        topBar = {
            SharedTopBar(
                title = "",
                onBack = onBack,
                actions = {
                    if (canDelete) {
                        IconButton(onClick = { confirmingDelete = true }, enabled = idle) {
                            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.action_delete))
                        }
                    }
                    if (canMark || canMove) {
                        var menuOpen by remember { mutableStateOf(false) }
                        Box {
                            IconButton(onClick = { menuOpen = true }) {
                                Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.more_options))
                            }
                            DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                                if (canMark && shared != null) {
                                    DropdownMenuItem(
                                        text = {
                                            Text(stringResource(if (shared.header.isUnread) R.string.action_mark_read else R.string.action_mark_unread))
                                        },
                                        enabled = idle,
                                        onClick = { menuOpen = false; c.act(SharedRules.toggleRead(shared.header)) },
                                    )
                                    DropdownMenuItem(
                                        text = { Text(stringResource(if (shared.header.isFlagged) R.string.action_unstar else R.string.action_star)) },
                                        enabled = idle,
                                        onClick = { menuOpen = false; c.act(SharedRules.toggleFlag(shared.header)) },
                                    )
                                }
                                if (canMove) {
                                    DropdownMenuItem(
                                        text = { Text(stringResource(R.string.action_move)) },
                                        enabled = idle,
                                        onClick = { menuOpen = false; moving = true },
                                    )
                                }
                            }
                        }
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            if (starting) androidx.compose.material3.LinearProgressIndicator(Modifier.fillMaxWidth())
            when {
                shared != null -> {
                    if (state.foldersFailed) {
                        Note(stringResource(R.string.shared_actions_failed), onRetry = c::refresh)
                    }
                    Text(
                        shared.header.subject.trim().ifEmpty { stringResource(R.string.list_no_subject) },
                        modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 4.dp, bottom = 12.dp),
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.Medium,
                    )
                    MessageCard(
                        header = shared.header,
                        shape = Groups.shape(0, 1),
                        expanded = true,
                        body = BodyState.Loaded(shared.body),
                        remoteAllowed = state.remoteAllowed,
                        myAddress = null,
                        busyBlobId = busyBlobId,
                        account = route.account,
                        actions = CardActions(
                            onToggle = {},
                            onRetryBody = c::refresh,
                            onAllowRemote = c::allowRemote,
                            onOpenLink = ::openLink,
                            onOpenAttachment = ::openAttachment,
                            onSaveAttachment = {
                                if (busyBlobId == null) {
                                    vm.pendingSave = it
                                    saver.launch(FileNames.safe(it.name))
                                }
                            },
                            onReply = { vm.drafts.start(DraftKind.Reply(shared.header.id)) },
                            onReplyAll = { vm.drafts.start(DraftKind.ReplyAll(shared.header.id)) },
                            onForward = { vm.drafts.start(DraftKind.Forward(shared.header.id)) },
                            showMenu = true,
                        ),
                    )
                    Spacer(Modifier.height(24.dp))
                }
                state.error != null -> Box(Modifier.fillMaxWidth().height(320.dp)) {
                    SharedCenteredBox { Note(stringResource(R.string.shared_message_failed) + "\n" + state.error!!.text(), onRetry = c::refresh) }
                }
                else -> Box(Modifier.fillMaxWidth().height(320.dp)) { SharedCenteredBox { Loading() } }
            }
        }
    }

    if (moving) {
        SharedMoveDialog(
            targets = state.moveTargets,
            onPick = {
                moving = false
                c.act(SharedAction.Move(it))
            },
            onDismiss = { moving = false },
        )
    }
    if (confirmingDelete) {
        AlertDialog(
            onDismissRequest = { confirmingDelete = false },
            title = { Text(stringResource(R.string.shared_delete_title)) },
            text = { Text(stringResource(R.string.shared_delete_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmingDelete = false
                    c.act(SharedAction.Delete)
                }) { Text(stringResource(R.string.shared_delete_confirm), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmingDelete = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** The shared folders of the account that take messages, as a plain list. */
@Composable
private fun SharedMoveDialog(targets: List<SharedFolderEntry>, onPick: (mailboxId: String) -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.action_move_title)) },
        text = {
            LazyColumn(Modifier.heightIn(max = 360.dp)) {
                items(targets, key = { it.mailbox.id }) { entry ->
                    Text(
                        entry.mailbox.name,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onPick(entry.mailbox.id) }
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
