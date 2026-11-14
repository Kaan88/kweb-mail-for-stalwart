package ovh.kweb.mail.ui.compose

import android.app.Application
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.ViewModelProvider.AndroidViewModelFactory.Companion.APPLICATION_KEY
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.thread.FileNames
import ovh.kweb.mail.ui.thread.SizeFormat
import ovh.kweb.mail.ui.thread.ThreadRules
import uniffi.kweb_core.Address

/**
 * Writing a message: the draft with [draftId] from the core. [onLeave] goes back, [onSent] goes
 * to the mail list, where the core's hold-back is shown with Undo.
 */
@Composable
fun ComposeScreen(draftId: String, onLeave: () -> Unit, onSent: () -> Unit) {
    val vm: ComposeViewModel = viewModel(
        key = "compose:$draftId",
        factory = viewModelFactory { initializer { ComposeViewModel(this[APPLICATION_KEY] as Application, draftId) } },
    )
    val s = vm.state
    val activity = LocalActivity.current
    val toFocus = remember { FocusRequester() }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetMultipleContents()) { uris: List<Uri> ->
        if (uris.isNotEmpty()) vm.attach(uris)
    }

    LaunchedEffect(vm) { vm.start() }
    // Leaving the screen for good saves what is left and lets go; a rotation keeps everything.
    DisposableEffect(vm) { onDispose { if (activity?.isChangingConfigurations != true) vm.close() } }
    // Android may kill the process once the app is in the background: write the draft now.
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { vm.flush() }
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            when (event) {
                ComposeEvent.Leave -> onLeave()
                ComposeEvent.Sent -> onSent()
            }
        }
    }
    // A new message starts in the To field. An answer is already addressed.
    val ready = s.ready
    LaunchedEffect(ready) {
        if (ready && s.to.isEmpty()) runCatching { toFocus.requestFocus() }
    }
    BackHandler { if (s.ready) vm.leave() else onLeave() }

    Scaffold(
        topBar = {
            TopBar(
                title = stringResource(
                    when (s.kind) {
                        ComposeRules.Kind.New -> R.string.compose_title_new
                        ComposeRules.Kind.Reply -> R.string.compose_title_reply
                        ComposeRules.Kind.Forward -> R.string.compose_title_forward
                    },
                ),
                canSend = s.canSend,
                canAttach = s.ready && !s.sending,
                onClose = { if (s.ready) vm.leave() else onLeave() },
                onAttach = { picker.launch("*/*") },
                onSend = vm::send,
            )
        },
        bottomBar = { StatusLine(s) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        when (s.phase) {
            Phase.Loading -> Box(Modifier.padding(padding).fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            Phase.Failed -> Column(
                Modifier.padding(padding).fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(stringResource(R.string.compose_load_failed), color = MaterialTheme.colorScheme.onSurfaceVariant)
                TextButton(onClick = vm::retry) { Text(stringResource(R.string.retry)) }
            }
            Phase.Missing -> Box(Modifier.padding(padding).fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                Text(stringResource(R.string.compose_missing), color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Phase.Ready -> Column(Modifier.padding(padding).fillMaxSize()) {
                Notices(s, vm)
                Form(s, vm, toFocus)
            }
        }
    }
}

@Composable
private fun TopBar(
    title: String,
    canSend: Boolean,
    canAttach: Boolean,
    onClose: () -> Unit,
    onAttach: () -> Unit,
    onSend: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background).statusBarsPadding().padding(horizontal = 8.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        IconButton(onClick = onClose) {
            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.compose_close))
        }
        Text(
            title,
            modifier = Modifier.weight(1f).padding(start = 8.dp),
            style = MaterialTheme.typography.titleLarge,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        IconButton(onClick = onAttach, enabled = canAttach) {
            Icon(painterResource(R.drawable.ic_attachment), contentDescription = stringResource(R.string.compose_attach))
        }
        // A filled pill with its icon, as the Compose figure has it.
        Button(
            onClick = onSend,
            enabled = canSend,
            shape = CircleShape,
            modifier = Modifier.height(40.dp),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 18.dp),
        ) {
            Icon(Icons.AutoMirrored.Filled.Send, contentDescription = null, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.compose_send), style = MaterialTheme.typography.labelLarge)
        }
    }
}

/** What went wrong, under the top bar where it is seen: error container for a failure, with its way out. */
@Composable
private fun Notices(s: ComposeState, vm: ComposeViewModel) {
    val error = s.error
    if (error != null) {
        Bar(error, if (s.canLeaveAnyway) stringResource(R.string.compose_leave_anyway) to vm::leaveAnyway else null)
    }
    val attachError = s.attachError
    if (attachError != null) Bar(attachError, stringResource(R.string.compose_dismiss) to vm::dismissAttachError)
}

@Composable
private fun Bar(text: String, action: Pair<String, () -> Unit>?) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(colors.errorContainer)
            .padding(start = 14.dp, end = if (action != null) 6.dp else 14.dp, top = 8.dp, bottom = 8.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, modifier = Modifier.weight(1f), color = colors.onErrorContainer, style = MaterialTheme.typography.labelLarge)
        if (action != null) {
            TextButton(onClick = action.second) { Text(action.first, color = colors.onErrorContainer) }
        }
    }
}

@Composable
private fun Form(s: ComposeState, vm: ComposeViewModel, toFocus: FocusRequester) {
    val colors = MaterialTheme.colorScheme
    val subjectFocus = remember { FocusRequester() }
    val bodyFocus = remember { FocusRequester() }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        if (s.identities.size > 1) {
            FromField(s.identities, s.identityId, vm::setIdentity, enabled = !s.sending)
            Line()
        }
        for (field in Field.entries) {
            if (field != Field.To && !s.ccBccOpen) continue
            RecipientField(
                label = stringResource(
                    when (field) {
                        Field.To -> R.string.compose_to
                        Field.Cc -> R.string.compose_cc
                        Field.Bcc -> R.string.compose_bcc
                    },
                ),
                addresses = s.addresses(field),
                text = s.text(field),
                marked = field in s.marked,
                suggestions = if (s.suggestionsFor == field) s.suggestions else emptyList(),
                onText = { vm.onText(field, it) },
                onCommit = { vm.commit(field) },
                onRemove = { vm.remove(field, it) },
                onPick = { vm.pick(field, it) },
                focusRequester = if (field == Field.To) toFocus else null,
                trailing = if (field == Field.To && !s.ccBccOpen) {
                    { CcBccToggle(vm::openCcBcc) }
                } else {
                    null
                },
            )
            Line()
        }
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 11.dp).heightIn(min = 26.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.compose_subject),
                modifier = Modifier.width(FieldLabelWidth),
                color = colors.onSurfaceVariant,
                style = MaterialTheme.typography.bodyLarge,
            )
            BasicTextField(
                value = s.subject,
                onValueChange = vm::setSubject,
                modifier = Modifier.weight(1f).focusRequester(subjectFocus),
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.onSurface),
                singleLine = true,
                cursorBrush = SolidColor(colors.primary),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Next),
                keyboardActions = KeyboardActions(onNext = { runCatching { bodyFocus.requestFocus() } }),
            )
        }
        Line()
        s.attachments.forEach { attachment ->
            val size = remember(attachment.size) { SizeFormat.format(attachment.size.toLong(), Locale.getDefault()) }
            FileCard(attachment.name, size, busy = false, onRemove = { vm.removeAttachment(attachment) })
        }
        s.attaching.forEach { FileCard(it, stringResource(R.string.compose_uploading), busy = true, onRemove = null) }
        BasicTextField(
            value = s.body,
            onValueChange = vm::setBody,
            modifier = Modifier.fillMaxWidth().defaultMinSize(minHeight = 320.dp).focusRequester(bodyFocus).padding(horizontal = 20.dp, vertical = 14.dp),
            textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.onSurface),
            cursorBrush = SolidColor(colors.primary),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            decorationBox = { inner ->
                Box {
                    if (s.body.isEmpty()) {
                        Text(stringResource(R.string.compose_body_hint), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
                    }
                    inner()
                }
            },
        )
    }
}

@Composable
private fun Line() = HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

/** The From line, only when the account has more than one address to send as. */
@Composable
private fun FromField(identities: List<uniffi.kweb_core.Identity>, selectedId: String, onPick: (String) -> Unit, enabled: Boolean) {
    var open by remember { mutableStateOf(false) }
    val colors = MaterialTheme.colorScheme
    val selected = identities.firstOrNull { it.id == selectedId } ?: identities.first()
    Box {
        Row(
            Modifier.fillMaxWidth().clickable(enabled = enabled) { open = true }.padding(horizontal = 20.dp, vertical = 11.dp).heightIn(min = 26.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(stringResource(R.string.compose_from), Modifier.width(FieldLabelWidth), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
            Text(
                ThreadRules.fullAddress(Address(selected.name, selected.email)),
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyLarge,
            )
            Icon(Icons.Filled.ArrowDropDown, contentDescription = stringResource(R.string.compose_choose_sender), tint = colors.onSurfaceVariant)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, shape = RoundedCornerShape(20.dp)) {
            identities.forEach { identity ->
                DropdownMenuItem(
                    text = {
                        Column {
                            if (identity.name.isNotBlank()) Text(identity.name, fontWeight = FontWeight.Bold)
                            Text(identity.email, style = MaterialTheme.typography.bodyMedium)
                        }
                    },
                    onClick = {
                        open = false
                        onPick(identity.id)
                    },
                )
            }
        }
    }
}

/** A file on the message: a tile with its kind, the name and the size, and a button to take it off. */
@Composable
private fun FileCard(name: String, detail: String, busy: Boolean, onRemove: (() -> Unit)?) {
    val colors = MaterialTheme.colorScheme
    val kind = remember(name) { FileNames.kind(name) }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 20.dp).padding(top = 12.dp).clip(RoundedCornerShape(20.dp)).background(colors.surfaceContainerHigh).padding(10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(42.dp).clip(RoundedCornerShape(12.dp)).background(colors.tertiaryContainer), contentAlignment = Alignment.Center) {
            when {
                busy -> CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = colors.onTertiaryContainer)
                kind != null -> Text(kind, color = colors.onTertiaryContainer, fontWeight = FontWeight.Bold, style = MaterialTheme.typography.labelSmall)
                else -> Icon(painterResource(R.drawable.ic_attachment), contentDescription = null, modifier = Modifier.size(20.dp), tint = colors.onTertiaryContainer)
            }
        }
        Column(Modifier.weight(1f)) {
            Text(name.ifBlank { stringResource(R.string.thread_attachment) }, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(detail, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
        }
        if (onRemove != null) {
            val label = stringResource(R.string.compose_remove_attachment, name)
            IconButton(onClick = onRemove) { Icon(Icons.Filled.Close, contentDescription = label) }
        }
    }
}

/** The quiet line under the form: when the draft was last saved. It also sits above the keyboard. */
@Composable
private fun StatusLine(s: ComposeState) {
    val colors = MaterialTheme.colorScheme
    val saved = s.savedAtMillis
    val text = when {
        s.saveFailed -> stringResource(R.string.compose_save_failed)
        saved != null -> {
            val time = remember(saved) {
                DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(Locale.getDefault())
                    .format(Instant.ofEpochMilli(saved).atZone(ZoneId.systemDefault()))
            }
            stringResource(R.string.compose_draft_saved, time)
        }
        else -> ""
    }
    Row(
        Modifier
            .fillMaxWidth()
            .background(colors.background)
            .windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars))
            .padding(horizontal = 20.dp, vertical = 8.dp)
            .heightIn(min = 20.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
    ) {
        Text(
            text,
            color = if (s.saveFailed) colors.error else colors.onSurfaceVariant,
            style = MaterialTheme.typography.labelMedium,
        )
    }
}
