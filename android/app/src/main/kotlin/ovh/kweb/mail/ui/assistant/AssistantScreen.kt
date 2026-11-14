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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
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
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiModel
import uniffi.kweb_core.AiProvider

/**
 * The settings of the AI assistant: the main switch, the service, address, model and key, what it may do, and Save.
 * [onBack] leaves, after a save and when the user lets changes go.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AssistantScreen(onBack: () -> Unit, vm: AssistantSettingsViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(vm) { vm.load() }
    LaunchedEffect(vm) { vm.saved.collect { onBack() } }

    // While a save runs back does nothing: leaving would cancel the coroutine that reports a failure.
    val leave = { if (state.saving) Unit else if (vm.isChanged) confirmDiscard = true else onBack() }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.assistant_title)) },
                navigationIcon = {
                    IconButton(onClick = leave, enabled = !state.saving) {
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
            val form = state.form
            when {
                state.phase == AssistantPhase.READY && form != null -> Form(state, form, vm)
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
private fun Form(state: AssistantSettingsState, form: AssistantRules.Form, vm: AssistantSettingsViewModel) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            SwitchRow(Groups.shape(0, 1), stringResource(R.string.assistant_switch), form.enabled, stringResource(R.string.assistant_switch_detail)) { v ->
                vm.edit { it.copy(enabled = v) }
            }
        }

        SectionLabel(stringResource(R.string.assistant_service))
        val services = listOf(
            AiProvider.OPEN_AI_COMPATIBLE to R.string.assistant_service_openai,
            AiProvider.ANTHROPIC to R.string.assistant_service_anthropic,
        )
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = 12.dp)) {
            services.forEachIndexed { index, (provider, label) ->
                SegmentedButton(
                    selected = form.provider == provider,
                    onClick = { vm.setProvider(provider) },
                    shape = SegmentedButtonDefaults.itemShape(index, services.size),
                    enabled = !state.saving,
                ) { Text(stringResource(label), maxLines = 1) }
            }
        }
        if (form.provider == AiProvider.OPEN_AI_COMPATIBLE) {
            Text(
                stringResource(R.string.assistant_service_openai_detail),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 8.dp),
                color = colors.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }

        val showEffort = EffortRules.isShown(form.provider)
        val count = if (showEffort) 4 else 3
        val ready = !state.switching
        GroupColumn(Modifier.padding(top = 16.dp)) {
            FieldRow(Groups.shape(0, count), stringResource(R.string.assistant_address), form.baseUrl, { v -> vm.edit { it.copy(baseUrl = v) } }, KeyboardType.Uri, ready)
            ModelRow(Groups.shape(1, count), form.model, ready, { v -> vm.edit { it.copy(model = v) } }, vm::chooseModel)
            if (showEffort) EffortRow(Groups.shape(2, count), state, form, ready) { e -> vm.edit { it.copy(effort = e) } }
            KeyRow(Groups.shape(count - 1, count), state, form, vm, ready)
        }

        SectionLabel(stringResource(R.string.assistant_allow))
        GroupColumn {
            SwitchRow(Groups.shape(0, 3), stringResource(R.string.assistant_allow_summaries), form.allowSummaries) { v -> vm.edit { it.copy(allowSummaries = v) } }
            SwitchRow(Groups.shape(1, 3), stringResource(R.string.assistant_allow_drafts), form.allowDrafts) { v -> vm.edit { it.copy(allowDrafts = v) } }
            SwitchRow(Groups.shape(2, 3), stringResource(R.string.assistant_allow_rewrites), form.allowRewrites) { v -> vm.edit { it.copy(allowRewrites = v) } }
        }

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

@Composable
private fun ChoiceRow(shape: Shape, title: String, detail: String?, selected: Boolean, onSelect: () -> Unit) {
    RowFrame(shape, Modifier.selectable(selected = selected, role = Role.RadioButton, onClick = onSelect)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            if (detail != null) {
                Text(detail, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
        }
        RadioButton(selected = selected, onClick = null)
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
private fun FieldRow(shape: Shape, label: String, value: String, onValue: (String) -> Unit, keyboardType: KeyboardType, enabled: Boolean = true) {
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = true,
        enabled = enabled,
        shape = shape,
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = keyboardType),
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
        AiEffort.LOW -> R.string.assistant_effort_low
        AiEffort.MEDIUM -> R.string.assistant_effort_medium
        AiEffort.HIGH -> R.string.assistant_effort_high
        AiEffort.X_HIGH -> R.string.assistant_effort_xhigh
        AiEffort.MAX -> R.string.assistant_effort_max
    },
)

/** The effort (Anthropic only): a row that opens a choice of the levels the model offers. */
@Composable
private fun EffortRow(shape: Shape, state: AssistantSettingsState, form: AssistantRules.Form, enabled: Boolean, onPick: (AiEffort) -> Unit) {
    var open by rememberSaveable { mutableStateOf(false) }
    val offered = EffortRules.offered(form.provider, form.model, state.modelList)
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
                    Text(state.message.resolve(LocalResources.current), color = colors.error, textAlign = TextAlign.Center)
                    TextButton(onClick = onRetry) { Text(stringResource(R.string.retry)) }
                }
                is ModelsState.Loaded -> {
                    if (ModelRules.isBuiltIn(state.list)) {
                        Text(
                            stringResource(R.string.assistant_models_built_in),
                            modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
                            color = colors.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
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
    val optional = form.provider == AiProvider.OPEN_AI_COMPATIBLE
    Column(Modifier.clip(shape).background(MaterialTheme.colorScheme.surfaceContainerLow)) {
        TextField(
            value = vm.keyText,
            onValueChange = vm::setKey,
            enabled = enabled,
            label = { Text(stringResource(if (state.hasKey) R.string.assistant_key_new else if (optional) R.string.assistant_key_optional else R.string.assistant_key)) },
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
