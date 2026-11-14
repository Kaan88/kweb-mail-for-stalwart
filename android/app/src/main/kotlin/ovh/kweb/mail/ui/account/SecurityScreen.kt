package ovh.kweb.mail.ui.account

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch
import uniffi.kweb_core.AccountSecurity
import uniffi.kweb_core.ConnectedApp
import uniffi.kweb_core.Credential
import uniffi.kweb_core.Passkey

/** How the user gets into the account: password, two-step codes, app passwords, API keys, passkeys and connected apps. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SecurityScreen(onBack: () -> Unit, vm: SecurityViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    // Read when the screen opens and again when it comes back.
    LifecycleResumeEffect(vm) {
        c.load()
        onPauseOrDispose { }
    }
    // What happened, said once.
    LaunchedEffect(state.info) {
        state.info?.let {
            snackbar.showSnackbar(it.resolve(resources))
            c.dismissInfo()
        }
    }
    SecureWindow(active = state.dialog?.showsSecret == true)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.security_title)) },
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
            val security = state.security
            when {
                state.phase == SecurityPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                state.phase == SecurityPhase.FAILED -> PageMessage(stringResource(R.string.security_load_failed), c::load)
                state.phase == SecurityPhase.NONE -> PageMessage(state.unsupported?.resolve(resources).orEmpty(), c::load)
                security != null -> SecurityList(security, state, c)
            }
        }
    }

    SecurityDialogs(state, c, snackbar)
}

@Composable
private fun SecurityList(security: AccountSecurity, state: SecurityState, c: SecurityController) {
    val locale = remember { Locale.getDefault() }
    val now = System.currentTimeMillis() / 1000
    val twoStepLabel = stringResource(R.string.security_two_step)
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
        AccountSectionTitle(stringResource(R.string.security_group_password))
        AccountGroup {
            AccountRow(
                Groups.shape(0, 2),
                stringResource(R.string.security_password_title),
                detail = stringResource(R.string.security_password_hint),
                leading = { Icon(Icons.Filled.Lock, contentDescription = null) },
                onClick = c::askChangePassword,
            )
            AccountRow(
                Groups.shape(1, 2),
                stringResource(R.string.security_two_step),
                detail = stringResource(R.string.security_two_step_detail),
                leading = { Icon(painterResource(R.drawable.ic_phone), contentDescription = null) },
                onClick = if (security.twoStep) c::askTwoStepOff else c::startTwoStep,
                trailing = {
                    KwebSwitch(
                        checked = security.twoStep,
                        onCheckedChange = { if (it) c.startTwoStep() else c.askTwoStepOff() },
                        modifier = Modifier.semantics { contentDescription = twoStepLabel },
                        enabled = !state.busy,
                    )
                },
            )
        }

        CredentialGroup(R.string.security_group_app_passwords, R.string.security_new_app_password, CredentialKind.APP_PASSWORD, security.appPasswords, locale, now, c)
        CredentialGroup(R.string.security_group_api_keys, R.string.security_new_api_key, CredentialKind.API_KEY, security.apiKeys, locale, now, c)

        AccountSectionTitle(stringResource(R.string.security_group_passkeys))
        AccountGroup {
            if (security.passkeys.isEmpty()) {
                AccountRow(Groups.shape(0, 1), stringResource(R.string.security_no_passkeys), quiet = true)
            }
            security.passkeys.forEachIndexed { i, passkey -> key(passkey.id) { PasskeyRow(Groups.shape(i, security.passkeys.size), passkey, locale, c) } }
        }

        AccountSectionTitle(stringResource(R.string.security_group_apps))
        AccountGroup {
            if (security.connectedApps.isEmpty()) {
                AccountRow(Groups.shape(0, 1), stringResource(R.string.security_no_apps), quiet = true)
            }
            security.connectedApps.forEachIndexed { i, app -> AppRow(Groups.shape(i, security.connectedApps.size), app, state, c) }
        }
        Text(
            stringResource(R.string.security_apps_note),
            modifier = Modifier.padding(start = 32.dp, end = 32.dp, top = 12.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun CredentialGroup(
    title: Int,
    newLabel: Int,
    kind: CredentialKind,
    list: List<Credential>,
    locale: Locale,
    now: Long,
    c: SecurityController,
) {
    AccountSectionTitle(stringResource(title))
    AccountGroup {
        val total = list.size + 1
        list.forEachIndexed { i, credential -> key(credential.id) {
            var menu by remember { mutableStateOf(false) }
            val line = listOfNotNull(
                AccountRules.date(credential.createdAt, locale)?.let { stringResource(R.string.security_made, it) },
                AccountRules.date(credential.expiresAt, locale)?.let {
                    stringResource(if (AccountRules.isEnded(credential.expiresAt, now)) R.string.security_ended else R.string.security_ends, it)
                },
            ).joinToString(" · ").ifEmpty { null }
            AccountRow(
                Groups.shape(i, total),
                credential.description.ifBlank { stringResource(R.string.security_no_note) },
                detail = line,
                trailing = {
                    Box {
                        IconButton(onClick = { menu = true }) {
                            Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.security_menu, credential.description.ifBlank { stringResource(R.string.security_no_note) }))
                        }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.account_delete)) }, onClick = { menu = false; c.askDeleteCredential(kind, credential) })
                        }
                    }
                },
                leading = { Icon(if (kind == CredentialKind.APP_PASSWORD) painterResource(R.drawable.ic_label) else painterResource(R.drawable.ic_files), contentDescription = null) },
            )
        } }
        AccountRow(
            Groups.shape(total - 1, total),
            stringResource(newLabel),
            leading = { Icon(Icons.Filled.Add, contentDescription = null, tint = MaterialTheme.colorScheme.primary) },
            onClick = { c.askNewCredential(kind) },
        )
    }
}

@Composable
private fun PasskeyRow(shape: Shape, key: Passkey, locale: Locale, c: SecurityController) {
    var menu by remember { mutableStateOf(false) }
    AccountRow(
        shape,
        key.name,
        leading = { Icon(Icons.Filled.Person, contentDescription = null) },
        detail = AccountRules.date(key.createdAt, locale)?.let { stringResource(R.string.security_made, it) },
        trailing = {
            Box {
                IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.security_menu, key.name)) }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text(stringResource(R.string.security_rename)) }, onClick = { menu = false; c.askRenamePasskey(key) })
                    DropdownMenuItem(text = { Text(stringResource(R.string.account_delete)) }, onClick = { menu = false; c.askDeletePasskey(key) })
                }
            }
        },
    )
}

@Composable
private fun AppRow(shape: Shape, app: ConnectedApp, state: SecurityState, c: SecurityController) {
    val reach = SecurityRules.scopes(app.scopes).map { part ->
        when (part) {
            is ScopePart.Known -> stringResource(scopeText(part.word))
            is ScopePart.Raw -> part.name
        }
    }
    AccountRow(
        shape,
        app.name,
        detail = if (reach.isEmpty()) null else stringResource(R.string.security_may_reach, reach.joinToString(", ")),
        titleContent = {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(app.name, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface, modifier = Modifier.weight(1f, fill = false))
                if (app.isThisApp) Pill(stringResource(R.string.security_this_app))
            }
        },
        trailing = if (SecurityRules.mayRevoke(app)) {
            {
                TextButton(onClick = { c.askRevoke(app) }, enabled = !state.busy) {
                    Text(stringResource(R.string.security_revoke), color = MaterialTheme.colorScheme.error)
                }
            }
        } else {
            null
        },
    )
}

private fun scopeText(word: ScopeWord): Int = when (word) {
    ScopeWord.MAIL -> R.string.security_scope_mail
    ScopeWord.CALENDARS -> R.string.security_scope_calendars
    ScopeWord.CONTACTS -> R.string.security_scope_contacts
    ScopeWord.FILES -> R.string.security_scope_files
    ScopeWord.SETTINGS -> R.string.security_scope_settings
    ScopeWord.SECURITY -> R.string.security_scope_security
    ScopeWord.FULL -> R.string.security_scope_full
    ScopeWord.STAY_SIGNED_IN -> R.string.security_scope_stay
}
