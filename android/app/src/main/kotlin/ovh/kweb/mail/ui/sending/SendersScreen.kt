package ovh.kweb.mail.ui.sending

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.IdentityDetail

/**
 * The addresses the user sends as: the list, and the form for one (in place of the list while it is open).
 * [onBack] leaves the screen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendersScreen(onBack: () -> Unit, vm: SendersViewModel = viewModel()) {
    val c = vm.controller
    val state = c.state
    LaunchedEffect(vm) { c.start() }
    LifecycleResumeEffect(vm) {
        c.load()
        onPauseOrDispose { }
    }

    val editing = state.editor != null
    val leave = { if (state.busy) Unit else if (editing) c.leaveEditor() else onBack() }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    val title = when {
                        state.editor == null -> R.string.senders_title
                        state.editor.isNew -> R.string.senders_add
                        else -> R.string.senders_edit
                    }
                    Text(stringResource(title))
                },
                navigationIcon = {
                    IconButton(onClick = leave, enabled = !state.busy) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let { NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.compose_dismiss) to c::dismissError) }
            state.info?.let { NoticeBar(it.resolve(LocalResources.current), error = false, action = stringResource(R.string.compose_dismiss) to c::dismissInfo) }
            val form = state.editor
            when {
                form != null -> SenderFormView(state, form, c)
                state.phase == SendersPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                state.phase == SendersPhase.FAILED -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(stringResource(R.string.senders_load_failed), color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    TextButton(onClick = c::load) { Text(stringResource(R.string.retry)) }
                }
                else -> SenderList(state, c)
            }
        }
    }

    when (state.dialog) {
        SendersDialog.Delete -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.senders_delete_title)) },
            text = { Text(stringResource(R.string.senders_delete_text, state.editor?.email.orEmpty())) },
            confirmButton = { TextButton(onClick = c::confirmDelete) { Text(stringResource(R.string.senders_delete), color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        SendersDialog.Discard -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.senders_discard_title)) },
            text = { Text(stringResource(R.string.senders_discard_text)) },
            confirmButton = { TextButton(onClick = c::discardEditor) { Text(stringResource(R.string.senders_discard)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.senders_keep_editing)) } },
        )
        null -> Unit
    }
}

@Composable
private fun SenderList(state: SendersState, c: SendersController) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
        Text(
            stringResource(R.string.senders_hint),
            modifier = Modifier.padding(horizontal = 28.dp, vertical = 8.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
        GroupColumn(Modifier.padding(top = 8.dp)) {
            state.items.forEachIndexed { index, item -> SenderRow(Groups.shape(index, state.items.size), item) { c.open(item.id) } }
        }
        FilledTonalButton(
            onClick = c::openNew,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 16.dp).heightIn(min = 52.dp),
        ) {
            Icon(Icons.Filled.Add, contentDescription = null)
            Text(stringResource(R.string.senders_add), modifier = Modifier.padding(start = 8.dp), style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun SenderRow(shape: Shape, item: IdentityDetail, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .clickable(role = Role.Button, onClick = onClick)
            .heightIn(min = 64.dp)
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(SendersRules.title(item), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            SendersRules.subtitle(item)?.let {
                Text(it, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        if (item.isDefault) {
            Text(
                stringResource(R.string.senders_default_mark),
                modifier = Modifier.clip(CircleShape).background(colors.secondaryContainer).padding(horizontal = 12.dp, vertical = 4.dp),
                color = colors.onSecondaryContainer,
                style = MaterialTheme.typography.labelMedium,
            )
        }
    }
}

@Composable
private fun SenderFormView(state: SendersState, form: SenderForm, c: SendersController) {
    val colors = MaterialTheme.colorScheme
    val problem = state.problem
    val enabled = !state.busy
    Column(
        Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        OutlinedTextField(
            value = form.name,
            onValueChange = c::setName,
            label = { Text(stringResource(R.string.senders_name)) },
            supportingText = {
                Text(
                    stringResource(R.string.senders_count, SendersRules.length(form.name), SendersRules.NAME_LIMIT),
                    color = if (problem == SendersRules.Problem.NameTooLong) colors.error else colors.onSurfaceVariant,
                )
            },
            isError = problem == SendersRules.Problem.NameTooLong,
            singleLine = true,
            enabled = enabled,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words),
            modifier = Modifier.fillMaxWidth(),
        )
        if (form.isNew) {
            OutlinedTextField(
                value = form.email,
                onValueChange = c::setEmail,
                label = { Text(stringResource(R.string.senders_email)) },
                supportingText = {
                    Text(
                        stringResource(
                            when (problem) {
                                SendersRules.Problem.NoEmail -> R.string.senders_email_needed
                                SendersRules.Problem.BadEmail -> R.string.senders_email_bad
                                else -> R.string.senders_email_hint
                            },
                        ),
                    )
                },
                isError = problem == SendersRules.Problem.NoEmail || problem == SendersRules.Problem.BadEmail,
                singleLine = true,
                enabled = enabled,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                modifier = Modifier.fillMaxWidth(),
            )
        } else {
            Column(Modifier.padding(horizontal = 4.dp)) {
                Text(stringResource(R.string.senders_email), color = colors.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
                Text(form.email, style = MaterialTheme.typography.bodyLarge)
                Text(stringResource(R.string.senders_email_fixed), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            }
        }
        OutlinedTextField(
            value = form.replyTo,
            onValueChange = c::setReplyTo,
            label = { Text(stringResource(R.string.senders_reply_to)) },
            supportingText = { Text(stringResource(if (problem == SendersRules.Problem.BadReplyTo) R.string.senders_email_bad else R.string.senders_reply_to_hint)) },
            isError = problem == SendersRules.Problem.BadReplyTo,
            singleLine = true,
            enabled = enabled,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = form.signature,
            onValueChange = c::setSignature,
            label = { Text(stringResource(R.string.senders_signature)) },
            supportingText = {
                Text(
                    stringResource(R.string.senders_count, SendersRules.length(form.signature), SendersRules.SIGNATURE_LIMIT),
                    color = if (problem == SendersRules.Problem.SignatureTooLong) colors.error else colors.onSurfaceVariant,
                )
            },
            isError = problem == SendersRules.Problem.SignatureTooLong,
            minLines = 4,
            maxLines = 12,
            enabled = enabled,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            modifier = Modifier.fillMaxWidth(),
        )
        Row(
            Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.senders_use_for_new), style = MaterialTheme.typography.bodyLarge)
                if (form.isDefault) {
                    Text(stringResource(R.string.senders_use_for_new_already), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
                }
            }
            Switch(checked = form.useForNew, onCheckedChange = c::setUseForNew, enabled = enabled && !form.isDefault)
        }
        Button(onClick = c::save, enabled = enabled, modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp)) {
            if (state.busy) CircularProgressIndicator(Modifier.padding(end = 8.dp).heightIn(max = 20.dp), strokeWidth = 2.dp, color = colors.onPrimary)
            Text(stringResource(R.string.senders_save), style = MaterialTheme.typography.labelLarge)
        }
        if (!form.isNew && form.mayDelete) {
            OutlinedButton(onClick = c::askDelete, enabled = enabled, modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp)) {
                Text(stringResource(R.string.senders_delete), color = colors.error, style = MaterialTheme.typography.labelLarge)
            }
        }
    }
}
