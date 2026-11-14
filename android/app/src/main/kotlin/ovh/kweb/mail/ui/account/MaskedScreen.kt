package ovh.kweb.mail.ui.account

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.background
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch
import uniffi.kweb_core.MaskedAddress

/** The masked addresses: the list with a switch for each, the count against the server's limit and the sheet that makes a new one. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MaskedScreen(onBack: () -> Unit, vm: MaskedViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    // The list is read again when the screen comes back (another device may have made or deleted one).
    LifecycleResumeEffect(vm) {
        c.load()
        onPauseOrDispose { }
    }
    val copiedText = stringResource(R.string.masked_copied)
    var copied by remember { androidx.compose.runtime.mutableIntStateOf(0) }
    LaunchedEffect(copied) { if (copied > 0 && !systemConfirmsCopy) snackbar.showSnackbar(copiedText) }
    val copy = rememberCopier(sensitive = false) { copied++ }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.masked_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let { NoticeBar(it.resolve(resources), error = true, action = stringResource(R.string.contacts_dismiss) to c::dismissError) }
            when (state.phase) {
                MaskedPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                MaskedPhase.FAILED -> PageMessage(stringResource(R.string.masked_load_failed), c::load)
                MaskedPhase.NONE -> PageMessage(state.unsupported?.resolve(resources).orEmpty(), c::load)
                MaskedPhase.READY -> MaskedList(state, c, copy)
            }
        }
    }

    state.dialog?.let { MaskedDialogs(it, state, c) }
    state.sheet?.let { MaskedSheetContent(it, state, c, copy) }
}

@Composable
private fun MaskedList(state: MaskedState, c: MaskedController, copy: (String) -> Unit) {
    val locale = remember { Locale.getDefault() }
    val count = state.addresses.size
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
        Text(
            stringResource(R.string.masked_lede),
            modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
        val limit = state.limit
        AccountSectionTitle(if (limit != null) stringResource(R.string.masked_count_of, count, limit) else pluralStringResource(R.plurals.masked_count, count, count))
        if (count == 0) {
            AccountGroup { AccountRow(Groups.shape(0, 1), stringResource(R.string.masked_empty), quiet = true) }
        } else {
            AccountGroup {
                state.addresses.forEachIndexed { index, address ->
                    key(address.id) {
                        MaskedRow(Groups.shape(index, count), address, locale, pending = address.id in state.pending, c, copy)
                    }
                }
            }
        }
        val full = state.atLimit
        Button(
            onClick = c::openSheet,
            enabled = !full && !state.busy,
            modifier = Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 14.dp),
        ) {
            Icon(Icons.Filled.Add, contentDescription = null)
            Text(stringResource(R.string.masked_new), modifier = Modifier.padding(start = 8.dp))
        }
        Text(
            if (full && limit != null) pluralStringResource(R.plurals.masked_limit_line, limit, limit) else stringResource(R.string.masked_receive_only),
            modifier = Modifier.padding(start = 32.dp, end = 32.dp, top = 12.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

/** One address: the whole address in a monospace face, the note and the day under it, the switch and the menu. */
@Composable
private fun MaskedRow(shape: androidx.compose.ui.graphics.Shape, address: MaskedAddress, locale: Locale, pending: Boolean, c: MaskedController, copy: (String) -> Unit) {
    var menu by remember { mutableStateOf(false) }
    val colors = MaterialTheme.colorScheme
    val date = AccountRules.shortDate(address.createdAt, locale)
    AccountRow(
        shape = shape,
        title = address.email,
        titleContent = {
            Column(Modifier.alpha(if (address.enabled) 1f else 0.5f)) {
                Text(address.email, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodyMedium, color = colors.onSurface)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        address.description.ifBlank { stringResource(R.string.masked_no_note) },
                        modifier = Modifier.weight(1f),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        fontStyle = if (address.description.isBlank()) FontStyle.Italic else null,
                        color = colors.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                    )
                    if (date != null) Text(date, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
                }
            }
        },
        trailing = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                val switchLabel = stringResource(R.string.masked_switch, address.email)
                KwebSwitch(
                    checked = address.enabled,
                    onCheckedChange = { c.setEnabled(address.id, it) },
                    modifier = Modifier.semantics { contentDescription = switchLabel },
                    enabled = !pending,
                )
                Box {
                    IconButton(onClick = { menu = true }) {
                        Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.masked_menu))
                    }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.masked_copy)) }, onClick = { menu = false; copy(address.email) })
                        DropdownMenuItem(text = { Text(stringResource(R.string.masked_change_note)) }, onClick = { menu = false; c.askNote(address) })
                        DropdownMenuItem(text = { Text(stringResource(R.string.account_delete)) }, onClick = { menu = false; c.askDelete(address) })
                    }
                }
            }
        },
    )
}

@Composable
private fun MaskedDialogs(dialog: MaskedDialog, state: MaskedState, c: MaskedController) {
    when (dialog) {
        is MaskedDialog.Delete -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.masked_delete_title)) },
            text = { Text(stringResource(R.string.masked_delete_text, dialog.address.email)) },
            confirmButton = {
                TextButton(onClick = c::confirmDelete, enabled = !state.busy) {
                    Text(stringResource(R.string.account_delete), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = c::dismissDialog, enabled = !state.busy) { Text(stringResource(R.string.cancel)) } },
        )
        is MaskedDialog.Note -> {
            var note by rememberSaveable(dialog.address.id) { mutableStateOf(dialog.address.description) }
            AlertDialog(
                onDismissRequest = c::dismissDialog,
                title = { Text(stringResource(R.string.masked_note_title)) },
                text = {
                    OutlinedTextField(
                        value = note,
                        onValueChange = { note = it },
                        label = { Text(stringResource(R.string.masked_field_for)) },
                        singleLine = true,
                        enabled = !state.busy,
                        modifier = Modifier.fillMaxWidth(),
                    )
                },
                confirmButton = { TextButton(onClick = { c.saveNote(note) }, enabled = !state.busy) { Text(stringResource(R.string.account_save)) } },
                dismissButton = { TextButton(onClick = c::dismissDialog, enabled = !state.busy) { Text(stringResource(R.string.cancel)) } },
            )
        }
    }
}

/** The bottom sheet: the form, then the address that was made, large, with a Copy button. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MaskedSheetContent(sheet: MaskedSheet, state: MaskedState, c: MaskedController, copy: (String) -> Unit) {
    // A lambda that captured `state` would be a new key on every change and the sheet would slide in again.
    val busy by androidx.compose.runtime.rememberUpdatedState(state.busy)
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true, confirmValueChange = { !busy })
    ModalBottomSheet(onDismissRequest = c::closeSheet, sheetState = sheetState, containerColor = MaterialTheme.colorScheme.surfaceContainer) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            when (sheet) {
                MaskedSheet.Make -> MakeForm(state, c)
                is MaskedSheet.Created -> CreatedView(sheet.address, c, copy)
            }
        }
    }
}

@Composable
private fun MakeForm(state: MaskedState, c: MaskedController) {
    var note by rememberSaveable { mutableStateOf("") }
    var site by rememberSaveable { mutableStateOf("") }
    var prefix by rememberSaveable { mutableStateOf("") }
    val prefixOk = MaskedRules.prefixIsValid(prefix)
    Text(stringResource(R.string.masked_sheet_title), style = MaterialTheme.typography.titleLarge)
    Text(stringResource(R.string.masked_sheet_lede), color = MaterialTheme.colorScheme.onSurfaceVariant)
    OutlinedTextField(
        value = note,
        onValueChange = { note = it },
        label = { Text(stringResource(R.string.masked_field_for)) },
        singleLine = true,
        enabled = !state.busy,
        modifier = Modifier.fillMaxWidth(),
    )
    OutlinedTextField(
        value = site,
        onValueChange = { site = it },
        label = { Text(stringResource(R.string.masked_field_site)) },
        singleLine = true,
        enabled = !state.busy,
        keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri),
        modifier = Modifier.fillMaxWidth(),
    )
    OutlinedTextField(
        value = prefix,
        onValueChange = { prefix = it },
        label = { Text(stringResource(R.string.masked_field_prefix)) },
        singleLine = true,
        enabled = !state.busy,
        isError = !prefixOk,
        supportingText = { Text(stringResource(if (prefixOk) R.string.masked_prefix_help else R.string.masked_prefix_bad)) },
        modifier = Modifier.fillMaxWidth(),
    )
    val limit = state.limit
    if (state.atLimit && limit != null) {
        Text(pluralStringResource(R.plurals.masked_limit_line, limit, limit), color = MaterialTheme.colorScheme.error)
    } else {
        state.sheetError?.let { Text(it.resolve(LocalResources.current), color = MaterialTheme.colorScheme.error) }
    }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
        TextButton(onClick = c::closeSheet, enabled = !state.busy) { Text(stringResource(R.string.cancel)) }
        TextButton(onClick = { c.create(note, site, prefix) }, enabled = prefixOk && !state.busy && !state.atLimit) { Text(stringResource(R.string.masked_create)) }
    }
}

@Composable
private fun androidx.compose.foundation.layout.ColumnScope.CreatedView(address: MaskedAddress, c: MaskedController, copy: (String) -> Unit) {
    val purpose = MaskedRules.purpose(address.description, address.forDomain)
    Text(stringResource(R.string.masked_created_title), style = MaterialTheme.typography.titleLarge)
    Text(
        address.email,
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceContainerHighest, RoundedCornerShape(20.dp))
            .padding(16.dp),
        fontFamily = FontFamily.Monospace,
        style = MaterialTheme.typography.titleMedium,
    )
    Text(
        listOfNotNull(purpose?.let { stringResource(R.string.masked_created_for, it) }, stringResource(R.string.masked_created_text)).joinToString(" "),
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    // The sheet covers the screen's snackbar, so the button itself says it was copied.
    var copied by remember { mutableStateOf(false) }
    FilledTonalButton(onClick = { copy(address.email); copied = true }, modifier = Modifier.fillMaxWidth()) {
        Text(stringResource(if (copied) R.string.masked_copied else R.string.masked_copy))
    }
    TextButton(onClick = c::closeSheet, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.account_done)) }
}
