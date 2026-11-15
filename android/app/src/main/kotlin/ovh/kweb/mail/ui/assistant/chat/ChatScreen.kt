package ovh.kweb.mail.ui.assistant.chat

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.assistant.AssistantRules.ChatGate
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.EmptyNote
import ovh.kweb.mail.ui.list.Groups

/**
 * The chat with the assistant ([ovh.kweb.mail.ui.Route.AssistantChat]). [onOpenThread] opens a message the
 * assistant points at, [onOpenDraft] a reply draft in the composer (nothing is sent), [onOpenSettings] the
 * assistant's settings.
 *
 * The assistant only reads; whatever it would change is a card that waits for Approve. The conversation is kept in
 * memory only and ends when the screen is left for good.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(
    onBack: () -> Unit,
    onOpenThread: (threadId: String, messageId: String) -> Unit,
    onOpenDraft: (draftId: String) -> Unit,
    onOpenSettings: () -> Unit,
) {
    // The store of this route builds it with (Application, AccountSession).
    val vm: ChatViewModel = viewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    val resources = LocalResources.current
    val words = rememberChatWords()
    val listState = rememberLazyListState()
    var menuOpen by remember { mutableStateOf(false) }

    // Coming back from the settings puts this screen into the composition again without resuming the activity.
    LaunchedEffect(vm) { vm.refresh() }
    LifecycleResumeEffect(vm) {
        vm.refresh()
        onPauseOrDispose { }
    }
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            when (event) {
                is ChatEvent.OpenThread -> onOpenThread(event.threadId, event.messageId)
                is ChatEvent.OpenDraft -> onOpenDraft(event.draftId)
            }
        }
    }

    // Follow what is being written: the end of the list stays in view while an answer arrives and when it is complete.
    val items = state.items
    val tail = (items.lastOrNull() as? ChatItem.Theirs)?.text?.length ?: 0
    LaunchedEffect(items.size, tail) {
        if (items.isNotEmpty()) listState.scrollToItem(items.size)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.chat_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                actions = {
                    Box {
                        IconButton(onClick = { menuOpen = true }) {
                            Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.more_options))
                        }
                        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.chat_new)) },
                                onClick = {
                                    menuOpen = false
                                    vm.newChat()
                                },
                            )
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.chat_settings)) },
                                onClick = {
                                    menuOpen = false
                                    onOpenSettings()
                                },
                            )
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).consumeWindowInsets(padding).fillMaxSize().imePadding()) {
            val gate = state.gate
            val openSettings = stringResource(R.string.chat_open_settings) to onOpenSettings
            when {
                state.settingsFailed -> NoticeBar(stringResource(R.string.chat_settings_failed), error = true, action = stringResource(R.string.retry) to vm::refresh)
                gate == ChatGate.OFF -> NoticeBar(stringResource(R.string.chat_gate_off), error = false, action = openSettings)
                gate == ChatGate.CHAT_OFF -> NoticeBar(stringResource(R.string.chat_gate_chat_off), error = false, action = openSettings)
                gate == ChatGate.NOT_AGREED -> NoticeBar(stringResource(R.string.chat_gate_not_agreed), error = false, action = openSettings)
            }

            Box(Modifier.weight(1f).fillMaxWidth()) {
                when {
                    gate == null && !state.settingsFailed -> CircularProgressIndicator(Modifier.align(Alignment.Center))
                    items.isEmpty() -> EmptyNote(
                        icon = R.drawable.ic_assistant,
                        text = stringResource(R.string.chat_empty_title),
                        modifier = Modifier.align(Alignment.Center),
                        hint = if (state.host.isBlank()) stringResource(R.string.chat_empty_text_no_host) else stringResource(R.string.chat_empty_text, state.host),
                    )
                    else -> LazyColumn(
                        Modifier.fillMaxSize(),
                        state = listState,
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                    ) {
                        itemsIndexed(items) { index, item ->
                            val (place, count) = ChatRules.chipRun(items, index)
                            // Things that belong together sit 3 dp apart; everything else has room around it.
                            if (index > 0) Spacer(Modifier.height(if (place > 0) 3.dp else 12.dp))
                            when (item) {
                                is ChatItem.Mine -> MineBubble(item.text)
                                is ChatItem.Theirs -> TheirsText(item)
                                is ChatItem.Steps -> StepsGroup(item.lines, active = state.running && index == items.lastIndex, words = words)
                                is ChatItem.Chip -> LinkChip(item.link, Groups.shape(place, count), busy = state.draftBusy) { vm.open(item.link) }
                                is ChatItem.Card -> ProposalCard(
                                    item,
                                    words,
                                    onApprove = { vm.approve(item.proposal.id) },
                                    onDecline = { vm.decline(item.proposal.id) },
                                    onUndo = { vm.undo(item.proposal.id) },
                                )
                                is ChatItem.Notice -> NoteLine(item.note)
                            }
                        }
                        // What the list scrolls to, so that the end of a long answer is in view.
                        item { Spacer(Modifier.height(1.dp)) }
                    }
                }
            }

            state.notice?.let {
                NoticeBar(it.resolve(resources), error = true, action = stringResource(R.string.assistant_dismiss) to vm::dismissNotice)
            }
            if (state.running) LinearProgressIndicator(Modifier.fillMaxWidth())
            InputPill(
                text = vm.input,
                onText = vm::onInput,
                enabled = gate == ChatGate.READY,
                running = state.running,
                canSend = ChatRules.canSend(gate, state.running, vm.input),
                onSend = vm::send,
                onStop = vm::stop,
            )
        }
    }
}
