package ovh.kweb.mail.ui.contacts

import ovh.kweb.mail.ui.accounts.LocalAccountSession
import android.app.Application
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import androidx.core.net.toUri
import android.util.Log
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
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
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.list.MailRow
import ovh.kweb.mail.ui.list.Preview
import ovh.kweb.mail.ui.list.RowContent
import ovh.kweb.mail.ui.list.Sender
import uniffi.kweb_core.Address
import uniffi.kweb_core.ContactDetail
import uniffi.kweb_core.Labeled
import uniffi.kweb_core.MessageHeader

/**
 * One contact: its name and organization, its mail addresses, phone numbers and postal addresses (a tap
 * does what each is for), the notes, and the recent mail with it. [onWrite] opens a draft, [onOpenMail] a conversation.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContactScreen(
    contactId: String,
    onBack: () -> Unit,
    onEdit: (contactId: String) -> Unit,
    onWrite: (draftId: String, to: Address) -> Unit,
    onOpenMail: (threadId: String, messageId: String) -> Unit,
) {
    // The view model belongs to this visit (the store of its route, see ScreenStores), so a new visit asks the core again.
    val app = LocalContext.current.applicationContext as Application
    val accountSession = LocalAccountSession.current
    val vm: ContactViewModel = viewModel(factory = viewModelFactory { initializer { ContactViewModel(app, accountSession, contactId) } })
    val state by vm.state.collectAsStateWithLifecycle()
    val detail = state.detail
    var asking by rememberSaveable { mutableStateOf(false) }
    val context = LocalContext.current

    LaunchedEffect(vm) { vm.load() }
    LaunchedEffect(vm) { vm.left.collect { onBack() } }
    LaunchedEffect(vm) { vm.draftStarted.collect { onWrite(it.draftId, it.to) } }

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
                    if (detail != null && detail.mayWrite) {
                        IconButton(onClick = { onEdit(detail.id) }, enabled = !state.busy) {
                            Icon(Icons.Filled.Edit, contentDescription = stringResource(R.string.contact_edit))
                        }
                        IconButton(onClick = { asking = true }, enabled = !state.busy) {
                            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.contact_delete))
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.message?.let {
                NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.contacts_dismiss) to vm::dismissMessage)
            }
            when {
                detail != null -> ContactDetails(
                    detail,
                    state.recent,
                    onWrite = vm::write,
                    onDial = { launch(context, Intent(Intent.ACTION_DIAL, ContactRules.dialUri(it).toUri()), R.string.contact_no_dialler) },
                    onMap = { launch(context, Intent(Intent.ACTION_VIEW, ContactRules.geoUri(it).toUri()), R.string.contact_no_map) },
                    onOpenMail = onOpenMail,
                )
                state.phase == ContactPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                else -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    val missing = state.phase == ContactPhase.MISSING
                    Text(
                        stringResource(if (missing) R.string.contact_missing else R.string.contact_load_failed),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                    if (!missing) TextButton(onClick = vm::retry) { Text(stringResource(R.string.retry)) }
                }
            }
        }
    }

    if (asking && detail != null) {
        AlertDialog(
            onDismissRequest = { asking = false },
            title = { Text(stringResource(R.string.contact_delete_title, detail.contact.name)) },
            text = { Text(stringResource(R.string.contact_delete_text)) },
            confirmButton = {
                TextButton(onClick = {
                    asking = false
                    vm.delete()
                }) { Text(stringResource(R.string.action_delete), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { asking = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** Starts [intent]; when no app can take it a short message says so. */
internal fun launch(context: Context, intent: Intent, missing: Int) {
    try {
        context.startActivity(intent)
    } catch (e: ActivityNotFoundException) {
        Log.w(Background.TAG, "no app for ${intent.action}")
        Toast.makeText(context, context.getString(missing), Toast.LENGTH_SHORT).show()
    }
}

/** The content of a contact screen. A contact of a shared address book is shown with [recent] empty. */
@Composable
internal fun ContactDetails(
    detail: ContactDetail,
    recent: List<MessageHeader>,
    onWrite: (String) -> Unit,
    onDial: (String) -> Unit,
    onMap: (String) -> Unit,
    onOpenMail: (threadId: String, messageId: String) -> Unit,
) {
    val contact = detail.contact
    val locale = rememberLocale()
    val resources = LocalResources.current
    val emails = remember(contact.emails) { contact.emails.filter { it.value.isNotBlank() } }
    val phones = remember(contact.phones) { contact.phones.filter { it.value.isNotBlank() } }
    val addresses = remember(contact.addresses) { contact.addresses.filter { it.value.isNotBlank() } }
    val notes = contact.notes.trim()

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp).padding(bottom = 20.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            ContactAvatar(ContactRules.initial(contact.name, locale), size = 96)
            Text(
                contact.name,
                modifier = Modifier.padding(top = 16.dp).semantics { heading() },
                style = MaterialTheme.typography.headlineMedium,
                textAlign = TextAlign.Center,
            )
            if (contact.organization.isNotBlank()) {
                Text(
                    contact.organization.trim(),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                    textAlign = TextAlign.Center,
                )
            }
        }
        ValueGroup(emails, R.drawable.ic_mail, onWrite)
        ValueGroup(phones, R.drawable.ic_phone, onDial)
        ValueGroup(addresses, R.drawable.ic_place, onMap)
        if (notes.isNotEmpty()) {
            GroupColumn(Modifier.padding(top = 16.dp)) {
                ValueRow(Groups.shape(0, 1), R.drawable.ic_notes, notes, quiet = stringResource(R.string.contact_notes), onClick = null)
            }
        }
        if (recent.isNotEmpty()) {
            Text(
                stringResource(R.string.contact_recent_mail).uppercase(locale),
                modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 24.dp, bottom = 8.dp).semantics { heading() },
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
                letterSpacing = 0.8.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            GroupColumn {
                recent.forEachIndexed { index, message ->
                    MailRow(
                        rowContent(message, resources.getString(R.string.list_unknown_sender)),
                        onClick = { onOpenMail(message.threadId, message.id) },
                        shape = Groups.shape(index, recent.size),
                    )
                }
            }
        }
    }
}

/** The values of one kind as one group; nothing when there are none. [onClick] gets the value that was tapped. */
@Composable
private fun ValueGroup(values: List<Labeled>, icon: Int, onClick: (String) -> Unit) {
    if (values.isEmpty()) return
    GroupColumn(Modifier.padding(top = 16.dp)) {
        values.forEachIndexed { index, item ->
            ValueRow(
                Groups.shape(index, values.size),
                icon,
                item.value.trim(),
                quiet = labelText(item.label),
                onClick = { onClick(item.value.trim()) },
            )
        }
    }
}

/** The words for a label: "Work", "Private", "Other" (nothing when the contact says nothing), or the user's own word. */
@Composable
internal fun labelText(label: String): String? = when (ContactRules.labelKind(label)) {
    ContactRules.LabelKind.WORK -> stringResource(R.string.contact_label_work)
    ContactRules.LabelKind.PRIVATE -> stringResource(R.string.contact_label_private)
    ContactRules.LabelKind.OTHER -> if (label.isBlank()) null else stringResource(R.string.contact_label_other)
    ContactRules.LabelKind.CUSTOM -> label.trim()
}

@Composable
private fun ValueRow(shape: Shape, icon: Int, text: String, quiet: String?, onClick: (() -> Unit)?) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
            .heightIn(min = 56.dp)
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(painterResource(icon), contentDescription = null, tint = colors.onSurfaceVariant, modifier = Modifier.size(24.dp))
        Column(Modifier.weight(1f)) {
            Text(text, style = MaterialTheme.typography.bodyLarge)
            if (!quiet.isNullOrEmpty()) Text(quiet, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

private fun rowContent(m: MessageHeader, unknown: String): RowContent = RowContent(
    sender = Sender.label(m.fromName, m.fromEmail) ?: unknown,
    initial = Sender.initial(m.fromName, m.fromEmail),
    subject = m.subject.trim(),
    preview = Preview.oneLine(m.preview),
    receivedAt = m.receivedAt,
    unread = m.isUnread,
    flagged = m.isFlagged,
    hasAttachment = m.hasAttachment,
)
