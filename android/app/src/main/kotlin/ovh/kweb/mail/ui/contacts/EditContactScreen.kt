package ovh.kweb.mail.ui.contacts

import android.app.Application
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.list.Groups

/**
 * The form for a new contact ([contactId] null) or for changing one. [onBack] leaves when the user lets
 * changes go; [onSaved] is called once the core has the contact.
 */
@Composable
fun EditContactScreen(contactId: String?, onBack: () -> Unit, onSaved: () -> Unit, account: String? = null) {
    // The view model belongs to this visit (the store of its route, see ScreenStores): it survives a rotation and goes with the screen.
    val app = LocalContext.current.applicationContext as Application
    val vm: EditContactViewModel = viewModel(factory = viewModelFactory { initializer { EditContactViewModel(app, contactId, account) } })
    val state by vm.state.collectAsStateWithLifecycle()
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }
    val form = state.form

    LaunchedEffect(vm) { vm.load() }
    LaunchedEffect(vm) { vm.saved.collect { onSaved() } }

    // While a save runs back does nothing: leaving would cancel the coroutine that reports a failure.
    val leave = { if (state.saving) Unit else if (state.dirty) confirmDiscard = true else onBack() }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            Row(
                Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background).statusBarsPadding().padding(horizontal = 8.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                IconButton(onClick = leave, enabled = !state.saving) {
                    Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.contact_close))
                }
                Text(
                    stringResource(if (state.creating) R.string.contact_new else R.string.contact_edit_title),
                    modifier = Modifier.weight(1f).padding(start = 8.dp),
                    style = MaterialTheme.typography.titleLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                // A filled pill, as the Compose screen has its Send.
                Button(
                    onClick = vm::save,
                    enabled = state.phase == EditContactPhase.READY && !state.saving,
                    shape = CircleShape,
                    modifier = Modifier.height(40.dp),
                    contentPadding = PaddingValues(horizontal = 22.dp),
                ) {
                    if (state.saving) {
                        CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
                    } else {
                        Text(stringResource(R.string.contact_save), style = MaterialTheme.typography.labelLarge)
                    }
                }
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let {
                NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.contacts_dismiss) to vm::dismissError)
            }
            when {
                form != null && state.phase == EditContactPhase.READY -> Form(form, state, vm::edit)
                state.phase == EditContactPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                else -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        stringResource(
                            when (state.phase) {
                                EditContactPhase.MISSING -> R.string.contact_missing
                                EditContactPhase.READ_ONLY -> R.string.contact_read_only
                                EditContactPhase.NO_BOOK -> R.string.contact_no_book
                                else -> R.string.contact_load_failed
                            },
                        ),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                    if (state.phase == EditContactPhase.FAILED) TextButton(onClick = vm::load) { Text(stringResource(R.string.retry)) }
                }
            }
        }
    }

    if (confirmDiscard) {
        AlertDialog(
            onDismissRequest = { confirmDiscard = false },
            title = { Text(stringResource(R.string.contact_discard_title)) },
            text = { Text(stringResource(R.string.contact_discard_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDiscard = false
                    onBack()
                }) { Text(stringResource(R.string.contact_discard)) }
            },
            dismissButton = { TextButton(onClick = { confirmDiscard = false }) { Text(stringResource(R.string.contact_keep_editing)) } },
        )
    }
}

@Composable
private fun Form(form: ContactForm, state: EditContactState, edit: ((ContactForm) -> ContactForm) -> Unit) {
    val locale = rememberLocale()
    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        Group {
            add { shape -> FieldRow(shape, stringResource(R.string.contact_given_name), form.givenName, { v -> edit { it.copy(givenName = v) } }, Kind.NAME) }
            add { shape -> FieldRow(shape, stringResource(R.string.contact_family_name), form.familyName, { v -> edit { it.copy(familyName = v) } }, Kind.NAME) }
            add { shape -> FieldRow(shape, stringResource(R.string.contact_organization), form.organization, { v -> edit { it.copy(organization = v) } }, Kind.NAME) }
        }
        if (state.books.size > 1) {
            Group(Modifier.padding(top = 16.dp)) {
                add { shape ->
                    val name = state.books.firstOrNull { it.id == form.addressBookId }?.name.orEmpty()
                    BookRow(shape, name) { close ->
                        state.books.forEach { book ->
                            DropdownMenuItem(
                                text = { Text(book.name) },
                                onClick = {
                                    edit { it.copy(addressBookId = book.id) }
                                    close()
                                },
                            )
                        }
                    }
                }
            }
        }
        ValueSection(
            title = stringResource(R.string.contact_emails),
            hint = stringResource(R.string.contact_email_address),
            addLabel = stringResource(R.string.contact_add_email),
            list = FormList.EMAILS,
            form = form,
            edit = edit,
            kind = Kind.EMAIL,
            locale = locale,
        )
        ValueSection(
            title = stringResource(R.string.contact_phones),
            hint = stringResource(R.string.contact_phone_number),
            addLabel = stringResource(R.string.contact_add_phone),
            list = FormList.PHONES,
            form = form,
            edit = edit,
            kind = Kind.PHONE,
            locale = locale,
        )
        ValueSection(
            title = stringResource(R.string.contact_addresses),
            hint = stringResource(R.string.contact_postal_address),
            addLabel = stringResource(R.string.contact_add_address),
            list = FormList.ADDRESSES,
            form = form,
            edit = edit,
            kind = Kind.POSTAL,
            locale = locale,
        )
        Group(Modifier.padding(top = 16.dp)) {
            add { shape -> FieldRow(shape, stringResource(R.string.contact_notes), form.notes, { v -> edit { it.copy(notes = v) } }, Kind.NOTES) }
        }
    }
}

/** One list of the form: a quiet title, its lines, and an "Add" row that is the last row of the group. */
@Composable
private fun ValueSection(
    title: String,
    hint: String,
    addLabel: String,
    list: FormList,
    form: ContactForm,
    edit: ((ContactForm) -> ContactForm) -> Unit,
    kind: Kind,
    locale: java.util.Locale,
) {
    val rows = ContactForms.rowsOf(form, list)
    Text(
        title.uppercase(locale),
        modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 24.dp, bottom = 8.dp).semantics { heading() },
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    val count = rows.size + 1
    GroupColumn {
        rows.forEachIndexed { index, row ->
            // A new line asked for focus would need the keyboard; the user taps into it.
            androidx.compose.runtime.key(row.id) {
                ValueLine(
                    shape = Groups.shape(index, count),
                    hint = hint,
                    row = row,
                    kind = kind,
                    onValue = { v -> edit { f -> ContactForms.changeRow(f, list, row.id) { it.copy(value = v) } } },
                    onLabel = { l -> edit { f -> ContactForms.changeRow(f, list, row.id) { it.copy(label = l) } } },
                    onRemove = { edit { f -> ContactForms.removeRow(f, list, row.id) } },
                )
            }
        }
        AddRow(Groups.shape(rows.size, count), addLabel) { edit { ContactForms.addRow(it, list) } }
    }
}

private enum class Kind { NAME, EMAIL, PHONE, POSTAL, NOTES }

@Composable
private fun rowFieldColors() = TextFieldDefaults.colors(
    focusedContainerColor = Color.Transparent,
    unfocusedContainerColor = Color.Transparent,
    focusedIndicatorColor = Color.Transparent,
    unfocusedIndicatorColor = Color.Transparent,
)

private fun keyboard(kind: Kind) = KeyboardOptions(
    capitalization = when (kind) {
        Kind.NAME, Kind.POSTAL -> KeyboardCapitalization.Words
        Kind.NOTES -> KeyboardCapitalization.Sentences
        else -> KeyboardCapitalization.None
    },
    keyboardType = when (kind) {
        Kind.EMAIL -> KeyboardType.Email
        Kind.PHONE -> KeyboardType.Phone
        else -> KeyboardType.Text
    },
)

/** A text field filling a whole row of a group. */
@Composable
private fun FieldRow(shape: Shape, label: String, value: String, onValue: (String) -> Unit, kind: Kind) {
    val container = MaterialTheme.colorScheme.surfaceContainerLow
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = kind != Kind.NOTES,
        minLines = if (kind == Kind.NOTES) 3 else 1,
        shape = shape,
        keyboardOptions = keyboard(kind),
        colors = TextFieldDefaults.colors(
            focusedContainerColor = container,
            unfocusedContainerColor = container,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** One mail address, phone number or postal address: its label (a menu), the text, and a remove button. */
@Composable
private fun ValueLine(shape: Shape, hint: String, row: FormRow, kind: Kind, onValue: (String) -> Unit, onLabel: (String) -> Unit, onRemove: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(shape).background(MaterialTheme.colorScheme.surfaceContainerLow).heightIn(min = 56.dp).padding(start = 8.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        LabelMenu(row.label, onLabel)
        TextField(
            value = row.value,
            onValueChange = onValue,
            label = { Text(hint) },
            singleLine = kind != Kind.POSTAL,
            keyboardOptions = keyboard(kind),
            colors = rowFieldColors(),
            modifier = Modifier.weight(1f),
        )
        IconButton(onClick = onRemove) {
            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.contact_remove))
        }
    }
}

/** The label of a line as a quiet button that opens Work, Private and Other. A word of the user's own is shown as it is. */
@Composable
private fun LabelMenu(label: String, onLabel: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val kind = ContactRules.labelKind(label)
    val text = when (kind) {
        ContactRules.LabelKind.WORK -> stringResource(R.string.contact_label_work)
        ContactRules.LabelKind.PRIVATE -> stringResource(R.string.contact_label_private)
        ContactRules.LabelKind.OTHER -> stringResource(R.string.contact_label_other)
        ContactRules.LabelKind.CUSTOM -> label.trim()
    }
    Box {
        TextButton(onClick = { open = true }, modifier = Modifier.widthIn(max = 96.dp)) {
            Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for ((choice, words) in listOf(
                ContactRules.LabelKind.WORK to R.string.contact_label_work,
                ContactRules.LabelKind.PRIVATE to R.string.contact_label_private,
                ContactRules.LabelKind.OTHER to R.string.contact_label_other,
            )) {
                DropdownMenuItem(
                    text = { Text(stringResource(words)) },
                    onClick = {
                        onLabel(ContactRules.labelValue(choice, label))
                        open = false
                    },
                )
            }
        }
    }
}

/** A row that shows the chosen address book and opens a menu of the others. */
@Composable
private fun BookRow(shape: Shape, value: String, items: @Composable (close: () -> Unit) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .clip(shape)
                .background(MaterialTheme.colorScheme.surfaceContainerLow)
                .clickable(role = Role.DropdownList, onClick = { open = true })
                .heightIn(min = 56.dp)
                .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.contact_address_book), modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
            Text(value, color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) { items { open = false } }
    }
}

@Composable
private fun AddRow(shape: Shape, label: String, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .clickable(role = Role.Button, onClick = onClick)
            .heightIn(min = 56.dp)
            .padding(horizontal = 20.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(Icons.Filled.Add, contentDescription = null, tint = colors.primary)
        Text(label, color = colors.primary, style = MaterialTheme.typography.bodyLarge)
    }
}

/** Collects the rows of one group, so that each knows its place and so its corners. */
private class GroupScope {
    val rows = mutableListOf<@Composable (Shape) -> Unit>()

    fun add(row: @Composable (Shape) -> Unit) {
        rows += row
    }
}

@Composable
private fun Group(modifier: Modifier = Modifier, content: GroupScope.() -> Unit) {
    val scope = GroupScope().apply(content)
    GroupColumn(modifier) {
        scope.rows.forEachIndexed { index, row -> row(Groups.shape(index, scope.rows.size)) }
    }
}
