package ovh.kweb.mail.ui.settings

import android.content.ActivityNotFoundException
import android.content.Intent
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
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import ovh.kweb.mail.BuildConfig
import ovh.kweb.mail.R
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
