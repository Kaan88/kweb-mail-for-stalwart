package ovh.kweb.mail.ui.settings

import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.BlackRules
import ovh.kweb.mail.ui.theme.KwebSwitch
import ovh.kweb.mail.ui.theme.ThemeChoice
import ovh.kweb.mail.ui.theme.ThemeMode
import ovh.kweb.mail.ui.theme.ThemeState
import ovh.kweb.mail.ui.theme.seedColor
import ovh.kweb.mail.ui.theme.themeModeName
import ovh.kweb.mail.ui.theme.themeName

/** "Appearance": the colours, light or dark, and a black page for dark. Every choice is applied at once, behind the screen. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AppearanceScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val choice by ThemeState.choice.collectAsStateWithLifecycle()
    val mode by ThemeState.mode.collectAsStateWithLifecycle()
    val black by ThemeState.pureBlack.collectAsStateWithLifecycle()
    val offered = ThemeChoice.offered(Build.VERSION.SDK_INT)
    val blackEnabled = BlackRules.switchEnabled(mode)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.appearance_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            SectionTitle(stringResource(R.string.appearance_colour))
            Group {
                offered.forEachIndexed { i, option ->
                    SettingsRow(
                        Groups.shape(i, offered.size),
                        stringResource(themeName(option)),
                        if (option == ThemeChoice.Wallpaper) stringResource(R.string.theme_wallpaper_hint) else null,
                        icon = { Dot(option) },
                        onClick = { ThemeState.choose(context, option) },
                        trailing = { RadioButton(selected = option == choice, onClick = null) },
                    )
                }
            }
            Preview()

            SectionTitle(stringResource(R.string.appearance_mode))
            Group {
                ThemeMode.entries.forEachIndexed { i, option ->
                    SettingsRow(
                        Groups.shape(i, ThemeMode.entries.size),
                        stringResource(themeModeName(option)),
                        onClick = { ThemeState.chooseMode(context, option) },
                        trailing = { RadioButton(selected = option == mode, onClick = null) },
                    )
                }
            }

            SectionTitle(stringResource(R.string.appearance_dark))
            Group {
                SettingsRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.appearance_black),
                    stringResource(if (blackEnabled) R.string.appearance_black_detail else R.string.appearance_black_light),
                    trailing = { KwebSwitch(checked = black, onCheckedChange = { ThemeState.choosePureBlack(context, it) }, enabled = blackEnabled) },
                    onClick = if (blackEnabled) ({ ThemeState.choosePureBlack(context, !black) }) else null,
                )
            }
        }
    }
}

/** The three containers of the chosen scheme, side by side. */
@Composable
private fun Preview() {
    val colors = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        listOf(
            Triple(colors.primaryContainer, colors.onPrimaryContainer, R.string.appearance_preview_primary),
            Triple(colors.secondaryContainer, colors.onSecondaryContainer, R.string.appearance_preview_secondary),
            Triple(colors.tertiaryContainer, colors.onTertiaryContainer, R.string.appearance_preview_tertiary),
        ).forEach { (fill, on, label) ->
            Box(
                Modifier.weight(1f).height(56.dp).clip(RoundedCornerShape(16.dp)).background(fill),
                contentAlignment = Alignment.Center,
            ) { Text(stringResource(label), color = on, style = MaterialTheme.typography.labelLarge) }
        }
    }
}

/** The colour a theme is known by; the wallpaper theme shows the phone's own primary colour. */
@Composable
private fun Dot(option: ThemeChoice) {
    val color: Color = seedColor(option)
        ?: if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            androidx.compose.material3.dynamicLightColorScheme(LocalContext.current).primary
        } else {
            MaterialTheme.colorScheme.primary
        }
    Box(Modifier.size(28.dp).clip(CircleShape).background(color).border(1.dp, MaterialTheme.colorScheme.outlineVariant, CircleShape))
}
