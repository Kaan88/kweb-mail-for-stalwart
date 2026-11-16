package ovh.kweb.mail.ui.crypto

import android.app.Application
import android.net.Uri
import android.util.Log
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.settings.Group
import ovh.kweb.mail.ui.settings.SectionTitle
import ovh.kweb.mail.ui.theme.KwebSwitch
import uniffi.kweb_core.KeyEntry
import uniffi.kweb_core.KeyKind
import uniffi.kweb_core.KeyOrigin
import uniffi.kweb_core.PrivateFile
import uniffi.kweb_core.Exception as CoreException

data class CryptoSettingsState(
    val enabled: Boolean = false,
    val keys: List<KeyEntry> = emptyList(),
    val loaded: Boolean = false,
    val failed: Boolean = false,
    /** A PKCS#12 file the user chose that waits for its passphrase. Its bytes are in the view model, in memory only. */
    val askingPassphrase: Boolean = false,
    val passphraseWrong: Boolean = false,
)

/** What the screen says once: a snackbar, in the user's words. */
sealed interface CryptoSettingsEvent {
    data class Added(val names: String) : CryptoSettingsEvent

    data class Refused(val reason: String) : CryptoSettingsEvent
}

/**
 * The keyring and the switch of one account. Reads and writes go to the core off the main thread. A key file is read
 * into memory here and handed over; nothing is kept in this class beyond the keys' public descriptions.
 */
class CryptoSettingsViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(CryptoSettingsState())
    private val eventsFlow = kotlinx.coroutines.channels.Channel<CryptoSettingsEvent>(kotlinx.coroutines.channels.Channel.BUFFERED)

    val state: StateFlow<CryptoSettingsState> get() = stateFlow
    val events get() = eventsFlow.receiveAsFlow()

    init {
        load()
    }

    fun load() {
        viewModelScope.launch {
            try {
                val (on, keys) = withContext(Dispatchers.IO) { session.core.cryptoEnabled() to session.core.cryptoKeys() }
                stateFlow.value = CryptoSettingsState(on, keys, loaded = true)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "loading the keys failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(loaded = true, failed = true) }
            }
        }
    }

    fun setEnabled(on: Boolean) = act { session.core.setCryptoEnabled(on) }

    fun remove(id: String) = act { session.core.cryptoRemoveKey(id) }

    fun forgetAll() = act { session.core.cryptoForgetAll() }

    /** Adds the public keys in a file the user picked. */
    fun importFile(uri: Uri) = import(KeyOrigin.FILE) {
        getApplication<Application>().contentResolver.openInputStream(uri)?.use { readUpTo(it, MAX_FILE) } ?: ByteArray(0)
    }

    private var pendingP12: ByteArray? = null

    /** A private key file: an OpenPGP key goes in as it is; a PKCS#12 file asks for its passphrase first. */
    fun importPrivateFile(uri: Uri) {
        viewModelScope.launch {
            try {
                val data = withContext(Dispatchers.IO) {
                    getApplication<Application>().contentResolver.openInputStream(uri)?.use { readUpTo(it, MAX_FILE) } ?: ByteArray(0)
                }
                when (withContext(Dispatchers.IO) { session.core.cryptoInspectPrivate(data) }) {
                    PrivateFile.PKCS12 -> {
                        pendingP12 = data
                        stateFlow.update { it.copy(askingPassphrase = true, passphraseWrong = false) }
                    }
                    else -> importPrivate(data, null)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading a key file failed: ${e.javaClass.simpleName}")
                eventsFlow.send(CryptoSettingsEvent.Refused(""))
            }
        }
    }

    /** The passphrase of the waiting PKCS#12 file. A wrong one asks again; the file stays in memory until it is imported or dropped. */
    fun submitPassphrase(passphrase: String) {
        val data = pendingP12 ?: return
        viewModelScope.launch {
            val refused = importPrivate(data, passphrase)
            if (refused == null) {
                pendingP12 = null
                stateFlow.update { it.copy(askingPassphrase = false) }
            } else {
                stateFlow.update { it.copy(askingPassphrase = true, passphraseWrong = true) }
            }
        }
    }

    fun dropPassphraseRequest() {
        pendingP12 = null
        stateFlow.update { it.copy(askingPassphrase = false, passphraseWrong = false) }
    }

    /** Returns null when the key was added, else the core's reason. */
    private suspend fun importPrivate(data: ByteArray, passphrase: String?): String? {
        return try {
            val added = withContext(Dispatchers.IO) { session.core.cryptoImportPrivate(data, passphrase) }
            eventsFlow.send(CryptoSettingsEvent.Added(added.joinToString(", ") { it.label }))
            load()
            null
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.Invalid) {
            if (passphrase == null) eventsFlow.send(CryptoSettingsEvent.Refused(e.msg))
            e.msg
        } catch (e: Exception) {
            Log.w(Background.TAG, "importing a private key failed: ${e.javaClass.simpleName}")
            eventsFlow.send(CryptoSettingsEvent.Refused(""))
            ""
        }
    }

    fun forgetPassphrase(id: String) = act { session.core.cryptoForgetPassphrase(id) }

    fun importText(text: String) = import(KeyOrigin.PASTED) { text.toByteArray() }

    private fun import(origin: KeyOrigin, read: () -> ByteArray) {
        viewModelScope.launch {
            try {
                val added = withContext(Dispatchers.IO) { session.core.cryptoImport(read(), origin) }
                eventsFlow.send(CryptoSettingsEvent.Added(added.joinToString(", ") { it.label }))
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                eventsFlow.send(CryptoSettingsEvent.Refused(e.msg))
            } catch (e: Exception) {
                Log.w(Background.TAG, "importing a key failed: ${e.javaClass.simpleName}")
                eventsFlow.send(CryptoSettingsEvent.Refused(""))
            }
            load()
        }
    }

    private fun act(work: () -> Unit) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { work() }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "a keyring change failed: ${e.javaClass.simpleName}")
            }
            load()
        }
    }

    private fun readUpTo(input: java.io.InputStream, max: Int): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (out.size() < max) {
            val n = input.read(buffer, 0, minOf(buffer.size, max - out.size()))
            if (n < 0) break
            out.write(buffer, 0, n)
        }
        return out.toByteArray()
    }

    private companion object {
        /** More than any key file; a bigger one is cut and the core refuses it as too large to be a key. */
        const val MAX_FILE = 1 shl 21
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CryptoSettingsScreen(onBack: () -> Unit, vm: CryptoSettingsViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = androidx.compose.ui.platform.LocalResources.current
    var removing by remember { mutableStateOf<KeyEntry?>(null) }
    var pasting by remember { mutableStateOf(false) }
    var forgetting by remember { mutableStateOf(false) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> if (uri != null) vm.importFile(uri) }
    val privatePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> if (uri != null) vm.importPrivateFile(uri) }

    LaunchedEffect(vm) {
        vm.events.collect { event ->
            val text = when (event) {
                is CryptoSettingsEvent.Added -> resources.getString(R.string.crypto_imported, event.names)
                is CryptoSettingsEvent.Refused -> resources.getString(R.string.crypto_import_failed, event.reason)
            }
            snackbar.showSnackbar(text)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.crypto_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Group {
                KeyRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.crypto_switch),
                    stringResource(R.string.crypto_switch_detail),
                    onClick = { vm.setEnabled(!state.enabled) },
                ) { KwebSwitch(checked = state.enabled, onCheckedChange = { vm.setEnabled(it) }) }
            }
            Text(
                stringResource(if (state.enabled) R.string.crypto_not_searchable else R.string.crypto_off_note),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (state.failed) {
                Text(stringResource(R.string.crypto_load_failed), modifier = Modifier.padding(horizontal = 32.dp), color = MaterialTheme.colorScheme.error)
            }

            val own = state.keys.filter { it.hasSecret }
            val others = state.keys.filter { !it.hasSecret }
            SectionTitle(stringResource(R.string.crypto_own_keys))
            Group {
                if (own.isEmpty()) {
                    KeyRow(Groups.shape(0, 1), stringResource(R.string.crypto_own_empty), null, quiet = true)
                }
                own.forEachIndexed { i, key -> KeyEntryRow(Groups.shape(i, own.size), key, onForget = { vm.forgetPassphrase(key.id) }) { removing = key } }
            }
            Group(Modifier.padding(top = 12.dp)) {
                KeyRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.crypto_import_own),
                    stringResource(R.string.crypto_import_own_detail),
                    onClick = { privatePicker.launch(arrayOf("*/*")) },
                )
            }

            SectionTitle(stringResource(R.string.crypto_other_keys))
            Group {
                if (others.isEmpty()) {
                    KeyRow(Groups.shape(0, 1), stringResource(R.string.crypto_other_empty), null, quiet = true)
                }
                others.forEachIndexed { i, key -> KeyEntryRow(Groups.shape(i, others.size), key) { removing = key } }
            }
            Group(Modifier.padding(top = 12.dp)) {
                KeyRow(
                    Groups.shape(0, 2),
                    stringResource(R.string.crypto_import_public),
                    stringResource(R.string.crypto_import_public_detail),
                    onClick = { picker.launch(arrayOf("*/*")) },
                )
                KeyRow(Groups.shape(1, 2), stringResource(R.string.crypto_paste_public), null, onClick = { pasting = true })
            }
            if (state.keys.isNotEmpty()) {
                Group(Modifier.padding(top = 16.dp)) {
                    KeyRow(Groups.shape(0, 1), stringResource(R.string.crypto_forget_all), null, titleColor = MaterialTheme.colorScheme.error, onClick = { forgetting = true })
                }
            }
        }
    }

    removing?.let { key ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.crypto_remove_title)) },
            text = { Text(stringResource(if (key.hasSecret) R.string.crypto_remove_own_body else R.string.crypto_remove_other_body)) },
            confirmButton = { TextButton(onClick = { vm.remove(key.id); removing = null }) { Text(stringResource(R.string.crypto_remove), color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    if (forgetting) {
        AlertDialog(
            onDismissRequest = { forgetting = false },
            title = { Text(stringResource(R.string.crypto_forget_title)) },
            text = { Text(stringResource(R.string.crypto_forget_body)) },
            confirmButton = { TextButton(onClick = { vm.forgetAll(); forgetting = false }) { Text(stringResource(R.string.crypto_remove), color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = { forgetting = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    if (state.askingPassphrase) {
        PassphraseDialog(
            keyName = "",
            wrong = state.passphraseWrong,
            onSubmit = { pass, _ -> vm.submitPassphrase(pass) },
            onDismiss = vm::dropPassphraseRequest,
            showKeep = false,
            body = stringResource(R.string.crypto_import_passphrase_body),
        )
    }
    if (pasting) PasteDialog(onAdd = { vm.importText(it); pasting = false }, onDismiss = { pasting = false })
}

@Composable
private fun PasteDialog(onAdd: (String) -> Unit, onDismiss: () -> Unit) {
    var text by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.crypto_paste_public)) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                placeholder = { Text(stringResource(R.string.crypto_paste_hint)) },
                modifier = Modifier.fillMaxWidth().heightIn(min = 160.dp),
            )
        },
        confirmButton = { TextButton(onClick = { onAdd(text) }, enabled = text.isNotBlank()) { Text(stringResource(R.string.crypto_paste_add)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

private val dateFormat: DateTimeFormatter = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)

/** One key: its name, addresses, fingerprint and where it came from, with Remove at the end. */
@Composable
private fun KeyEntryRow(shape: Shape, key: KeyEntry, onForget: () -> Unit = {}, onRemove: () -> Unit) {
    val kind = stringResource(if (key.kind == KeyKind.PGP) R.string.crypto_kind_pgp else R.string.crypto_kind_smime)
    val origin = when (key.origin) {
        KeyOrigin.FILE -> stringResource(R.string.crypto_from_file)
        KeyOrigin.PASTED -> stringResource(R.string.crypto_from_paste)
        KeyOrigin.MAIL -> stringResource(R.string.crypto_from_mail, key.originDetail)
        KeyOrigin.OWN -> stringResource(R.string.crypto_from_own)
    }
    val expires = key.expiresAt?.let { stringResource(R.string.crypto_expires, dateFormat.withLocale(androidx.compose.ui.text.intl.Locale.current.platformLocale).format(Instant.ofEpochSecond(it).atZone(ZoneId.systemDefault()))) }
    val detail = listOfNotNull(
        kind + " · " + key.emails.joinToString(", "),
        stringResource(R.string.crypto_fingerprint, key.fingerprint),
        origin,
        expires,
        if (key.hasSecret && key.needsPassphrase) stringResource(if (key.unlocked) R.string.crypto_unlocked else R.string.crypto_locked) else null,
    ).joinToString("\n")
    KeyRow(shape, key.label, detail) {
        if (key.unlocked) TextButton(onClick = onForget) { Text(stringResource(R.string.crypto_forget_passphrase)) }
        IconButton(onClick = onRemove) { Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.crypto_remove)) }
    }
}

@Composable
private fun KeyRow(
    shape: Shape,
    title: String,
    detail: String?,
    quiet: Boolean = false,
    titleColor: androidx.compose.ui.graphics.Color = androidx.compose.ui.graphics.Color.Unspecified,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
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
        Column(Modifier.weight(1f)) {
            Text(
                title,
                color = if (quiet) colors.onSurfaceVariant else if (titleColor == androidx.compose.ui.graphics.Color.Unspecified) colors.onSurface else titleColor,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = if (quiet) null else FontWeight.Medium,
            )
            if (detail != null) Text(detail, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
        if (trailing != null) trailing()
    }
}
