package ovh.kweb.mail.ui.settings

import android.content.ActivityNotFoundException
import android.content.Intent
import android.text.format.DateUtils
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import ovh.kweb.mail.BuildConfig
import ovh.kweb.mail.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.UpdateChecker
import ovh.kweb.mail.background.UpdateScheduler
import ovh.kweb.mail.ui.UiPrefs
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.coreVersion

/** Where the source is. Public address, no credentials. */
internal const val SOURCE_URL = "https://github.com/Kaan88/kweb-mail-for-stalwart"

/** The libraries the app is built on and the licence of each, as shown under "Licences of libraries". */
internal val LIBRARY_LICENCES: List<Pair<String, String>> = listOf(
    "Jetpack Compose, AndroidX, Material Components" to "Apache 2.0",
    "Kotlin and kotlinx.coroutines" to "Apache 2.0",
    "JNA (calls into the core)" to "Apache 2.0 or LGPL 2.1",
    "UniFFI, Tokio, Rustls, Hyper, Serde, Tracing" to "MPL 2.0, MIT or Apache 2.0",
    "SQLite (through rusqlite)" to "Public domain, MIT",
    "rPGP, RustCrypto and the other crates of the encryption and signing code" to "MIT or Apache 2.0",
    "Material Symbols and Icons" to "Apache 2.0",
    "Filter rule format compatible with Bulwark webmail (bulwarkmail/webmail)" to "AGPL-3.0, with an app store permission",
    "Device calendar sync: the design follows Bulwark Mobile (bulwarkmail/native); design only, no code was copied" to "AGPL-3.0, with an app store permission",
)

/**
 * About: the version of the app and of its core, the licence, the source and the licences of the libraries.
 * Nothing here reads the account or the network.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AboutScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    var showLibraries by remember { mutableStateOf(false) }
    val core = remember { runCatching { coreVersion() }.getOrNull() }
    val prefs = remember { UiPrefs(context) }
    val scope = rememberCoroutineScope()
    val genericFailure = stringResource(R.string.update_failed_generic)
    var checking by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    var weekly by remember { mutableStateOf(prefs.updateWeekly) }
    // What the last check saw, read again after each check.
    var lastChecked by remember { mutableStateOf(prefs.updateLastChecked) }
    var latest by remember { mutableStateOf(prefs.updateLatest) }
    var newer by remember { mutableStateOf(prefs.updateNewer) }
    var pageUrl by remember { mutableStateOf(prefs.updatePageUrl) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.about_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            val rows = if (core != null) 4 else 3
            Group(Modifier.padding(top = 8.dp)) {
                var at = 0
                SettingsRow(
                    Groups.shape(at++, rows),
                    stringResource(R.string.about_version),
                    stringResource(R.string.about_version_value, BuildConfig.VERSION_NAME, BuildConfig.VERSION_CODE),
                )
                if (core != null) SettingsRow(Groups.shape(at++, rows), stringResource(R.string.about_core), core)
                SettingsRow(Groups.shape(at++, rows), stringResource(R.string.about_licence), stringResource(R.string.about_licence_value))
                SettingsRow(
                    Groups.shape(at, rows),
                    stringResource(R.string.about_source),
                    SOURCE_URL.removePrefix("https://"),
                    onClick = {
                        try {
                            context.startActivity(Intent(Intent.ACTION_VIEW, SOURCE_URL.toUri()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                        } catch (_: ActivityNotFoundException) {
                            // No browser: nothing to open.
                        }
                    },
                )
            }
            val download = newer && latest.isNotEmpty() && UpdateChecker.pageUri(pageUrl) != null
            val updateRows = if (download) 3 else 2
            Group(Modifier.padding(top = 16.dp)) {
                val detail = when {
                    checking -> stringResource(R.string.update_checking)
                    failure != null -> stringResource(R.string.update_failed, failure.orEmpty())
                    lastChecked == 0L -> stringResource(R.string.update_never)
                    latest.isEmpty() -> stringResource(R.string.update_last_none, DateUtils.getRelativeTimeSpanString(lastChecked).toString())
                    else -> stringResource(R.string.update_last, DateUtils.getRelativeTimeSpanString(lastChecked).toString(), latest)
                }
                SettingsRow(
                    Groups.shape(0, updateRows),
                    stringResource(R.string.update_check),
                    detail,
                    onClick = if (checking) null else ({
                        checking = true
                        failure = null
                        scope.launch {
                            val result = withContext(Dispatchers.IO) { UpdateChecker.run(context) }
                            checking = false
                            result.onSuccess { UpdateChecker.announce(context, it) }
                            result.onFailure { failure = (it as? uniffi.kweb_core.Exception.Temporary)?.msg ?: genericFailure }
                            lastChecked = prefs.updateLastChecked
                            latest = prefs.updateLatest
                            newer = prefs.updateNewer
                            pageUrl = prefs.updatePageUrl
                        }
                    }),
                    trailing = if (checking) ({ CircularProgressIndicator(Modifier.padding(4.dp), strokeWidth = 2.dp) }) else null,
                )
                if (download) {
                    SettingsRow(
                        Groups.shape(1, updateRows),
                        stringResource(R.string.update_download, latest),
                        stringResource(R.string.update_download_detail),
                        onClick = {
                            UpdateChecker.pageUri(pageUrl)?.let { page ->
                                try {
                                    context.startActivity(Intent(Intent.ACTION_VIEW, page).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                                } catch (_: ActivityNotFoundException) {
                                    // No browser: nothing to open.
                                }
                            }
                        },
                    )
                }
                SettingsRow(
                    Groups.shape(updateRows - 1, updateRows),
                    stringResource(R.string.update_weekly),
                    stringResource(R.string.update_weekly_detail),
                    onClick = {
                        weekly = !weekly
                        prefs.updateWeekly = weekly
                        UpdateScheduler.apply(context, weekly)
                    },
                    trailing = { Switch(checked = weekly, onCheckedChange = null) },
                )
            }
            Group(Modifier.padding(top = 16.dp)) {
                SettingsRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.about_libraries),
                    stringResource(R.string.about_libraries_detail),
                    onClick = { showLibraries = !showLibraries },
                )
            }
            if (showLibraries) {
                Group(Modifier.padding(top = 16.dp)) {
                    LIBRARY_LICENCES.forEachIndexed { index, (name, licence) ->
                        SettingsRow(Groups.shape(index, LIBRARY_LICENCES.size), name, licence)
                    }
                }
            }
        }
    }
}
