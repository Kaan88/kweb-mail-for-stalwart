package ovh.kweb.mail.ui.assistant

import android.view.View
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.theme.KwebSwitch
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.AiActionMode
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiModel
import uniffi.kweb_core.AiService

/**
 * The settings of the AI assistant: the main switch, the service, address, model and key, what it may do, and Save.
 * [onBack] leaves, after a save and when the user lets changes go.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AssistantScreen(onBack: () -> Unit, onOpenHomeAttention: () -> Unit = {}, vm: AssistantSettingsViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }
    var confirmReset by rememberSaveable { mutableStateOf(false) }
    var confirmChats by rememberSaveable { mutableStateOf(false) }
    var confirmRemove by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(vm) { vm.load() }
    LaunchedEffect(vm) { vm.saved.collect { onBack() } }

    // While a save runs back does nothing: leaving would cancel the coroutine that reports a failure.
    val leave = { if (state.saving || state.resetting || state.removing) Unit else if (vm.isChanged) confirmDiscard = true else onBack() }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.assistant_title)) },
                navigationIcon = {
                    IconButton(onClick = leave, enabled = !state.saving && !state.resetting && !state.removing) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let {
                NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.assistant_dismiss) to vm::dismissError)
            }
            state.info?.let {
                NoticeBar(it.resolve(LocalResources.current), error = false, action = stringResource(R.string.assistant_dismiss) to vm::dismissInfo)
            }
            val form = state.form
            when {
                state.phase == AssistantPhase.READY && form != null -> Form(
                    state,
                    form,
                    vm,
                    onOpenHomeAttention,
                    onDeleteChats = { confirmChats = true },
                    onRemoveService = { confirmRemove = true },
                    onReset = { confirmReset = true },
                )
                state.phase == AssistantPhase.FAILED -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(stringResource(R.string.assistant_load_failed), color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    TextButton(onClick = vm::load) { Text(stringResource(R.string.retry)) }
                }
                else -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            }
        }
    }

    state.consent?.let { ask ->
        AlertDialog(
            onDismissRequest = vm::declineConsent,
            title = { Text(stringResource(R.string.assistant_consent_settings_title, ask.host)) },
            text = { Text(stringResource(R.string.assistant_consent_settings_text, ask.host, ask.model.ifBlank { stringResource(R.string.assistant_model_none) })) },
            confirmButton = { TextButton(onClick = vm::agreeConsent, enabled = !ask.busy) { Text(stringResource(R.string.assistant_consent_agree)) } },
            dismissButton = { TextButton(onClick = vm::declineConsent, enabled = !ask.busy) { Text(stringResource(R.string.assistant_consent_decline)) } },
        )
    }

    state.chatConsent?.let { ask ->
        AlertDialog(
            onDismissRequest = vm::declineChatConsent,
            title = { Text(stringResource(R.string.assistant_chat_consent_title)) },
            text = { Text(stringResource(R.string.assistant_chat_consent_text, ask.host, ask.model.ifBlank { stringResource(R.string.assistant_model_none) })) },
            confirmButton = { TextButton(onClick = vm::agreeChatConsent, enabled = !ask.busy) { Text(stringResource(R.string.assistant_consent_agree)) } },
            dismissButton = { TextButton(onClick = vm::declineChatConsent, enabled = !ask.busy) { Text(stringResource(R.string.assistant_consent_decline)) } },
        )
    }

    if (confirmReset) {
        AlertDialog(
            onDismissRequest = { confirmReset = false },
            title = { Text(stringResource(R.string.assistant_reset_title)) },
            text = { Text(stringResource(R.string.assistant_reset_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmReset = false
                    vm.reset()
                }) { Text(stringResource(R.string.assistant_reset_confirm)) }
            },
            dismissButton = { TextButton(onClick = { confirmReset = false }) { Text(stringResource(R.string.assistant_keep_editing)) } },
        )
    }

    val removeId = state.form?.serviceId.orEmpty()
    if (confirmRemove && removeId.isNotEmpty()) {
        val shown = state.form
        val label = shown?.serviceName?.trim().orEmpty().ifEmpty { AssistantRules.hostOf(shown?.baseUrl.orEmpty()) }
        AlertDialog(
            onDismissRequest = { confirmRemove = false },
            title = { Text(stringResource(R.string.assistant_service_remove_title, label)) },
            text = { Text(stringResource(R.string.assistant_service_remove_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmRemove = false
                    vm.removeService(removeId)
                }) { Text(stringResource(R.string.assistant_service_remove_confirm)) }
            },
            dismissButton = { TextButton(onClick = { confirmRemove = false }) { Text(stringResource(R.string.assistant_keep_editing)) } },
        )
    }

    if (confirmChats) {
        AlertDialog(
            onDismissRequest = { confirmChats = false },
            title = { Text(stringResource(R.string.assistant_chats_delete_title)) },
            text = { Text(stringResource(R.string.assistant_chats_delete_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmChats = false
                    vm.deleteChats()
                }) { Text(stringResource(R.string.assistant_chats_delete_confirm)) }
            },
            dismissButton = { TextButton(onClick = { confirmChats = false }) { Text(stringResource(R.string.assistant_keep_editing)) } },
        )
    }

    state.models?.let { ModelsSheet(it, onPick = vm::pickModel, onRetry = vm::chooseModel, onClose = vm::closeModels) }

    if (confirmDiscard) {
        AlertDialog(
            onDismissRequest = { confirmDiscard = false },
            title = { Text(stringResource(R.string.assistant_discard_title)) },
            text = { Text(stringResource(R.string.assistant_discard_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDiscard = false
                    onBack()
                }) { Text(stringResource(R.string.assistant_discard)) }
            },
            dismissButton = { TextButton(onClick = { confirmDiscard = false }) { Text(stringResource(R.string.assistant_keep_editing)) } },
        )
    }
}

@Composable
private fun Form(
    state: AssistantSettingsState,
    form: AssistantRules.Form,
    vm: AssistantSettingsViewModel,
    onOpenHomeAttention: () -> Unit,
    onDeleteChats: () -> Unit,
    onRemoveService: () -> Unit,
    onReset: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            SwitchRow(Groups.shape(0, 1), stringResource(R.string.assistant_switch), form.enabled, switchDetail(form)) { v ->
                vm.edit { it.copy(enabled = v) }
            }
        }

        // The list of services: tap one to edit it; saving makes the shown one the service in use.
        if (state.services.isNotEmpty()) {
            SectionLabel(stringResource(R.string.assistant_services))
            val canAdd = AssistantRules.canAddService(state.services.size)
            val rowCount = state.services.size + (if (canAdd) 1 else 0)
            val listEnabled = !state.saving && !state.removing && !state.resetting
            GroupColumn {
                state.services.forEachIndexed { index, entry ->
                    ServiceEntryRow(Groups.shape(index, rowCount), entry, selected = entry.id == form.serviceId, enabled = listEnabled) {
                        vm.selectService(entry.id)
                    }
                }
                if (canAdd) AddServiceRow(Groups.shape(rowCount - 1, rowCount), enabled = listEnabled, onClick = vm::addService)
            }
        }

        SectionLabel(stringResource(if (form.serviceId.isEmpty()) R.string.assistant_service_new else R.string.assistant_service_selected))

        val offeredEfforts = EffortRules.offered(form.model, state.modelList, form.effort)
        val showEffort = EffortRules.isShown(offeredEfforts)
        // "Other address" stays chosen while the user types, even if what is typed is a named service's address.
        var pickedOther by rememberSaveable(form.serviceId) { mutableStateOf(false) }
        val service = AssistantRules.shownService(form.baseUrl, pickedOther)
        val showAddress = service == AssistantRules.Service.OTHER
        // Name, service, [address], model, [effort], key.
        val count = 4 + (if (showAddress) 1 else 0) + (if (showEffort) 1 else 0)
        val ready = !state.switching && !state.removing
        GroupColumn {
            var at = 0
            FieldRow(
                Groups.shape(at++, count),
                stringResource(R.string.assistant_service_name),
                form.serviceName,
                { v -> vm.edit { it.copy(serviceName = v) } },
                KeyboardType.Text,
                ready,
                placeholder = stringResource(R.string.assistant_service_name_hint),
                capitalization = KeyboardCapitalization.Words,
            )
            ServiceRow(Groups.shape(at++, count), service, ready && !state.saving) { picked ->
                pickedOther = picked == AssistantRules.Service.OTHER
                vm.edit { it.copy(baseUrl = AssistantRules.addressForService(it.baseUrl, picked)) }
            }
            if (showAddress) {
                FieldRow(Groups.shape(at++, count), stringResource(R.string.assistant_address), form.baseUrl, { v -> vm.edit { it.copy(baseUrl = v) } }, KeyboardType.Uri, ready)
            }
            ModelRow(Groups.shape(at++, count), form.model, ready, { v -> vm.edit { it.copy(model = v) } }, vm::chooseModel)
            if (showEffort) EffortRow(Groups.shape(at++, count), offeredEfforts, form, ready) { e -> vm.edit { it.copy(effort = e) } }
            KeyRow(Groups.shape(count - 1, count), state, form, vm, ready)
        }

        if (AssistantRules.showToolsNote(state.tools, form)) {
            Text(
                stringResource(R.string.assistant_tools_note),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 8.dp),
                color = colors.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }

        if (AssistantRules.isPrivateAddress(form.baseUrl)) {
            Text(
                stringResource(R.string.assistant_local_detail),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 8.dp),
                color = colors.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        if (AssistantRules.isCleartext(form.baseUrl)) {
            Text(
                stringResource(R.string.assistant_cleartext_warning),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 4.dp),
                color = colors.error,
                style = MaterialTheme.typography.bodyMedium,
            )
        }

        SectionLabel(stringResource(R.string.assistant_allow))
        GroupColumn {
            SwitchRow(Groups.shape(0, 7), stringResource(R.string.assistant_allow_summaries), form.allowSummaries) { v -> vm.edit { it.copy(allowSummaries = v) } }
            SwitchRow(Groups.shape(1, 7), stringResource(R.string.assistant_allow_drafts), form.allowDrafts) { v -> vm.edit { it.copy(allowDrafts = v) } }
            SwitchRow(Groups.shape(2, 7), stringResource(R.string.assistant_allow_rewrites), form.allowRewrites) { v -> vm.edit { it.copy(allowRewrites = v) } }
            SwitchRow(Groups.shape(3, 7), stringResource(R.string.assistant_allow_ideas), form.allowIdeas) { v -> vm.edit { it.copy(allowIdeas = v) } }
            SwitchRow(Groups.shape(4, 7), stringResource(R.string.assistant_allow_extract), form.allowExtract) { v -> vm.edit { it.copy(allowExtract = v) } }
            SwitchRow(Groups.shape(5, 7), stringResource(R.string.assistant_allow_explain), form.allowExplain) { v -> vm.edit { it.copy(allowExplain = v) } }
            SwitchRow(Groups.shape(6, 7), stringResource(R.string.assistant_allow_chat), form.allowChat, stringResource(R.string.assistant_allow_chat_detail)) { v ->
                vm.edit { it.copy(allowChat = v) }
            }
        }

        // The sparkle buttons of Home and the other screens follow the switches above; their own settings are one tap away.
        ovh.kweb.mail.ui.attention.HomeAttentionRow(onOpenHomeAttention, Modifier.padding(top = 16.dp))

        // What the chat may do about changes. There is no "automatic": a change is only ever suggested and waits for a tap.
        SectionLabel(stringResource(R.string.assistant_actions))
        GroupColumn {
            ChoiceRow(
                Groups.shape(0, 2),
                stringResource(R.string.assistant_actions_off),
                stringResource(R.string.assistant_actions_off_detail),
                selected = form.actions == AiActionMode.OFF,
                enabled = form.allowChat,
            ) { vm.edit { it.copy(actions = AiActionMode.OFF) } }
            ChoiceRow(
                Groups.shape(1, 2),
                stringResource(R.string.assistant_actions_ask),
                stringResource(R.string.assistant_actions_ask_detail),
                selected = form.actions == AiActionMode.ASK_FIRST,
                enabled = form.allowChat,
            ) { vm.edit { it.copy(actions = AiActionMode.ASK_FIRST) } }
        }

        // The user's own instructions: sent as an extra system text with every request.
        SectionLabel(stringResource(R.string.assistant_instructions))
        GroupColumn {
            TextField(
                value = form.instructions,
                onValueChange = { v -> vm.edit { it.copy(instructions = v.take(AssistantRules.INSTRUCTIONS_MAX)) } },
                label = { Text(stringResource(R.string.assistant_instructions_label)) },
                supportingText = { Text(stringResource(R.string.assistant_instructions_count, form.instructions.length, AssistantRules.INSTRUCTIONS_MAX)) },
                minLines = 3,
                maxLines = 8,
                shape = Groups.shape(0, 1),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                colors = fieldColors(),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Text(
            stringResource(R.string.assistant_instructions_help),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 8.dp),
            color = colors.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )

        Text(
            stringResource(R.string.assistant_privacy),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 16.dp),
            color = colors.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )

        // The main action: a filled pill, 52 dp high.
        Button(
            onClick = vm::save,
            enabled = !state.saving && !state.switching,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).height(52.dp),
        ) {
            if (state.saving) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = colors.onPrimary)
            } else {
                Text(stringResource(R.string.assistant_save), style = MaterialTheme.typography.labelLarge)
            }
        }

        // Only a stored service can be removed (its key and agreement go with it).
        if (form.serviceId.isNotEmpty()) {
            TextButton(
                onClick = onRemoveService,
                enabled = !state.saving && !state.switching && !state.removing && !state.resetting,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
            ) {
                if (state.removing) {
                    CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                } else {
                    Text(stringResource(R.string.assistant_service_remove), color = colors.error)
                }
            }
        }

        // Earlier chats are kept on this phone; this removes them.
        Text(
            stringResource(R.string.assistant_chats_help),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 8.dp),
            color = colors.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
        TextButton(
            onClick = onDeleteChats,
            enabled = !state.saving && !state.switching && !state.resetting,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
        ) { Text(stringResource(R.string.assistant_chats_delete)) }

        // Quiet, below the main action: it throws the settings, the keys and the agreements away.
        TextButton(
            onClick = onReset,
            enabled = !state.saving && !state.switching && !state.resetting,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            if (state.resetting) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            } else {
                Text(stringResource(R.string.assistant_reset), color = colors.error)
            }
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    Text(
        text.uppercase(),
        modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun RowFrame(shape: Shape, modifier: Modifier = Modifier, color: Color = MaterialTheme.colorScheme.surfaceContainerLow, content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit) {
    Row(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(color)
            .heightIn(min = 56.dp)
            .padding(horizontal = 20.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) { content() }
}

@Composable
private fun SwitchRow(shape: Shape, title: String, checked: Boolean, detail: String? = null, onChange: (Boolean) -> Unit) {
    RowFrame(shape, Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onChange)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            if (detail != null) {
                Text(detail, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
        }
        KwebSwitch(checked = checked, onCheckedChange = null)
    }
}

/** One service of the list: its name, its host and model, and a mark when it is the one in use. [selected] is the one shown below. */
@Composable
private fun ServiceEntryRow(shape: Shape, entry: AiService, selected: Boolean, enabled: Boolean, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    RowFrame(shape, Modifier.selectable(selected = selected, enabled = enabled, role = Role.RadioButton, onClick = onClick)) {
        Column(Modifier.weight(1f)) {
            Text(entry.name.ifBlank { AssistantRules.hostOf(entry.baseUrl) }, style = MaterialTheme.typography.bodyLarge)
            val detail = AssistantRules.serviceDetail(entry)
            if (detail.isNotEmpty()) {
                Text(detail, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
        }
        if (entry.active) {
            Text(stringResource(R.string.assistant_service_in_use), color = colors.primary, style = MaterialTheme.typography.labelMedium)
        }
        RadioButton(selected = selected, onClick = null, enabled = enabled)
    }
}

@Composable
private fun AddServiceRow(shape: Shape, enabled: Boolean, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    RowFrame(shape, Modifier.clickable(enabled = enabled, role = Role.Button, onClick = onClick)) {
        Icon(Icons.Filled.Add, contentDescription = null, tint = colors.primary)
        Text(stringResource(R.string.assistant_service_add), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
    }
}

/** One choice of a group of which exactly one is selected. A row that is not [enabled] is shown faded and takes no tap. */
@Composable
private fun ChoiceRow(shape: Shape, title: String, detail: String?, selected: Boolean, enabled: Boolean = true, onSelect: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    // The faded tones Material gives disabled content, taken from the scheme.
    val titleColor = if (enabled) colors.onSurface else colors.onSurface.copy(alpha = 0.38f)
    val detailColor = if (enabled) colors.onSurfaceVariant else colors.onSurfaceVariant.copy(alpha = 0.38f)
    RowFrame(shape, Modifier.selectable(selected = selected, enabled = enabled, role = Role.RadioButton, onClick = onSelect)) {
        Column(Modifier.weight(1f)) {
            Text(title, color = titleColor, style = MaterialTheme.typography.bodyLarge)
            if (detail != null) {
                Text(detail, color = detailColor, style = MaterialTheme.typography.bodyMedium)
            }
        }
        RadioButton(selected = selected, onClick = null, enabled = enabled)
    }
}

@Composable
private fun fieldColors(): androidx.compose.material3.TextFieldColors {
    val container = MaterialTheme.colorScheme.surfaceContainerLow
    return TextFieldDefaults.colors(
        focusedContainerColor = container,
        unfocusedContainerColor = container,
        focusedIndicatorColor = Color.Transparent,
        unfocusedIndicatorColor = Color.Transparent,
    )
}

@Composable
private fun FieldRow(
    shape: Shape,
    label: String,
    value: String,
    onValue: (String) -> Unit,
    keyboardType: KeyboardType,
    enabled: Boolean = true,
    placeholder: String? = null,
    capitalization: KeyboardCapitalization = KeyboardCapitalization.None,
) {
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        placeholder = if (placeholder == null) null else { { Text(placeholder) } },
        singleLine = true,
        enabled = enabled,
        shape = shape,
        keyboardOptions = KeyboardOptions(capitalization = capitalization, autoCorrectEnabled = false, keyboardType = keyboardType),
        colors = fieldColors(),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** The model: a field to type into, with "Choose" to pick from what the service offers. */
@Composable
private fun ModelRow(shape: Shape, value: String, enabled: Boolean, onValue: (String) -> Unit, onChoose: () -> Unit) {
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(stringResource(R.string.assistant_model)) },
        singleLine = true,
        enabled = enabled,
        shape = shape,
        trailingIcon = { TextButton(onClick = onChoose, enabled = enabled) { Text(stringResource(R.string.assistant_model_choose)) } },
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = KeyboardType.Ascii),
        colors = fieldColors(),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun effortLabel(effort: AiEffort): String = stringResource(
    when (effort) {
        AiEffort.DEFAULT -> R.string.assistant_effort_default
        AiEffort.OFF -> R.string.assistant_effort_off
        AiEffort.MINIMAL -> R.string.assistant_effort_minimal
        AiEffort.LOW -> R.string.assistant_effort_low
        AiEffort.MEDIUM -> R.string.assistant_effort_medium
        AiEffort.HIGH -> R.string.assistant_effort_high
        AiEffort.X_HIGH -> R.string.assistant_effort_xhigh
        AiEffort.MAX -> R.string.assistant_effort_max
    },
)

/** The effort: a row that opens a choice of the levels the service listed for the model ([offered]). */
@Composable
private fun EffortRow(shape: Shape, offered: List<AiEffort>, form: AssistantRules.Form, enabled: Boolean, onPick: (AiEffort) -> Unit) {
    var open by rememberSaveable { mutableStateOf(false) }
    RowFrame(shape, Modifier.clickable(enabled = enabled, role = Role.Button) { open = true }) {
        Column(Modifier.weight(1f)) {
            Text(stringResource(R.string.assistant_effort), style = MaterialTheme.typography.bodyLarge)
            Text(effortLabel(form.effort), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
    }
    if (open) {
        AlertDialog(
            onDismissRequest = { open = false },
            title = { Text(stringResource(R.string.assistant_effort)) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    Text(
                        stringResource(R.string.assistant_effort_detail),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(bottom = 8.dp),
                    )
                    offered.forEach { level ->
                        Row(
                            Modifier.fillMaxWidth().selectable(selected = form.effort == level, role = Role.RadioButton) {
                                onPick(level)
                                open = false
                            }.heightIn(min = 48.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            RadioButton(selected = form.effort == level, onClick = null)
                            Text(effortLabel(level), style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { open = false }) { Text(stringResource(R.string.assistant_close)) } },
        )
    }
}

/** The models the service offers, in a bottom sheet: loading, the list, or what went wrong. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ModelsSheet(state: ModelsState, onPick: (AiModel) -> Unit, onRetry: () -> Unit, onClose: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    ModalBottomSheet(onDismissRequest = onClose, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), containerColor = colors.surfaceContainerLow) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
            Text(
                stringResource(R.string.assistant_models_title),
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                style = MaterialTheme.typography.titleLarge,
            )
            when (state) {
                ModelsState.Loading -> Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                is ModelsState.Failed -> Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    val failure = state.failure
                    Text(
                        when (failure.kind) {
                            ModelFailure.Kind.KEY_REFUSED -> stringResource(R.string.assistant_models_key_refused)
                            ModelFailure.Kind.UNREACHABLE -> stringResource(R.string.assistant_models_unreachable, failure.host.orEmpty())
                            ModelFailure.Kind.OTHER -> stringResource(R.string.assistant_models_failed)
                        },
                        color = colors.error,
                        textAlign = TextAlign.Center,
                    )
                    failure.details?.let { details ->
                        var shown by remember { mutableStateOf(false) }
                        TextButton(onClick = { shown = !shown }) { Text(stringResource(R.string.assistant_models_details)) }
                        if (shown) Text(details, style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, textAlign = TextAlign.Center)
                    }
                    TextButton(onClick = onRetry) { Text(stringResource(R.string.retry)) }
                }
                is ModelsState.Loaded -> {
                    if (state.list.models.isEmpty()) {
                        Text(
                            stringResource(R.string.assistant_models_none),
                            modifier = Modifier.padding(24.dp),
                            color = colors.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                    LazyColumn(Modifier.heightIn(max = 480.dp).padding(horizontal = 12.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                        val models = state.list.models
                        items(models.size) { index ->
                            val model = models[index]
                            RowFrame(Groups.shape(index, models.size), Modifier.clickable(role = Role.Button) { onPick(model) }, colors.surfaceContainerHighest) {
                                Column(Modifier.weight(1f)) {
                                    Text(ModelRules.title(model), style = MaterialTheme.typography.bodyLarge)
                                    ModelRules.subtitle(model)?.let {
                                        Text(it, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/**
 * The key: "A key is stored" with Remove when there is one, a hidden field otherwise. The stored key is never shown and
 * never put in the field. The field uses a password keyboard, and while it is on screen the window is kept out of
 * autofill, so that no service offers to save or fill it.
 */
/** The line under the main switch: what the assistant is set to, or that it is off (and whether it was ever set up). */
@Composable
private fun switchDetail(form: AssistantRules.Form): String = when (val line = AssistantRules.switchLine(form)) {
    AssistantRules.SwitchLine.NotSetUp -> stringResource(R.string.assistant_switch_detail)
    AssistantRules.SwitchLine.Off -> stringResource(R.string.assistant_switch_off)
    AssistantRules.SwitchLine.On -> stringResource(R.string.assistant_switch_on)
    is AssistantRules.SwitchLine.Using -> {
        val name = when {
            line.name.isNotEmpty() -> line.name
            line.service == AssistantRules.Service.OTHER -> line.host.ifEmpty { stringResource(R.string.assistant_svc_other) }
            else -> stringResource(serviceName(line.service))
        }
        stringResource(R.string.assistant_switch_using, name, line.model)
    }
}

internal fun serviceName(service: AssistantRules.Service): Int = when (service) {
    AssistantRules.Service.OPENAI -> R.string.assistant_svc_openai
    AssistantRules.Service.OPENROUTER -> R.string.assistant_svc_openrouter
    AssistantRules.Service.GEMINI -> R.string.assistant_svc_gemini
    AssistantRules.Service.MISTRAL -> R.string.assistant_svc_mistral
    AssistantRules.Service.DEEPSEEK -> R.string.assistant_svc_deepseek
    AssistantRules.Service.GROQ -> R.string.assistant_svc_groq
    AssistantRules.Service.OTHER -> R.string.assistant_svc_other
}

/** The service of the OpenAI-style protocol: a named one fills in its address, "Other address" shows the field. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ServiceRow(shape: Shape, selected: AssistantRules.Service, enabled: Boolean, onPick: (AssistantRules.Service) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = open && enabled, onExpandedChange = { open = it && enabled }) {
        TextField(
            value = stringResource(serviceName(selected)),
            onValueChange = {},
            readOnly = true,
            enabled = enabled,
            label = { Text(stringResource(R.string.assistant_service)) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
            shape = shape,
            colors = fieldColors(),
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(expanded = open && enabled, onDismissRequest = { open = false }) {
            AssistantRules.Service.entries.forEach { service ->
                DropdownMenuItem(
                    text = { Text(stringResource(serviceName(service))) },
                    onClick = {
                        open = false
                        onPick(service)
                    },
                    contentPadding = ExposedDropdownMenuDefaults.ItemContentPadding,
                )
            }
        }
    }
}

@Composable
private fun KeyRow(shape: Shape, state: AssistantSettingsState, form: AssistantRules.Form, vm: AssistantSettingsViewModel, enabled: Boolean) {
    val keepStored = state.hasKey && !form.removeKey
    if (keepStored) {
        RowFrame(shape) {
            Text(stringResource(R.string.assistant_key_stored), modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
            TextButton(onClick = { vm.removeKey(true) }, enabled = enabled) { Text(stringResource(R.string.assistant_key_remove)) }
        }
        return
    }
    val view = LocalView.current
    DisposableEffect(view) {
        val before = view.importantForAutofill
        view.importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS
        onDispose { view.importantForAutofill = before }
    }
    Column(Modifier.clip(shape).background(MaterialTheme.colorScheme.surfaceContainerLow)) {
        TextField(
            value = vm.keyText,
            onValueChange = vm::setKey,
            enabled = enabled,
            label = { Text(stringResource(if (state.hasKey) R.string.assistant_key_new else R.string.assistant_key_optional)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = KeyboardType.Password),
            colors = fieldColors(),
            modifier = Modifier.fillMaxWidth(),
        )
        if (state.hasKey) {
            Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 8.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    stringResource(R.string.assistant_key_removing),
                    modifier = Modifier.weight(1f),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                )
                TextButton(onClick = { vm.removeKey(false); vm.setKey("") }, enabled = enabled) { Text(stringResource(R.string.assistant_key_keep)) }
            }
        }
    }
}
