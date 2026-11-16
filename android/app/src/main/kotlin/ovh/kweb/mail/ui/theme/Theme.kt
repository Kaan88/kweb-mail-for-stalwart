package ovh.kweb.mail.ui.theme

import android.content.Context
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import ovh.kweb.mail.ui.UiPrefs

/** The theme the app is drawn in. One value for the process; the picker changes it and it is remembered. */
object ThemeState {
    private val flow = MutableStateFlow(ThemeChoice.default(Build.VERSION.SDK_INT))
    private val modeFlow = MutableStateFlow(ThemeMode.System)
    private val blackFlow = MutableStateFlow(false)

    val choice: StateFlow<ThemeChoice> get() = flow
    val mode: StateFlow<ThemeMode> get() = modeFlow
    val pureBlack: StateFlow<Boolean> get() = blackFlow

    /** Reads the remembered choice. Called before the first frame, so the app never flashes another theme. */
    fun load(context: Context) {
        val prefs = UiPrefs(context)
        flow.value = ThemeChoice.fromKey(prefs.theme, Build.VERSION.SDK_INT)
        modeFlow.value = ThemeMode.of(prefs.themeMode)
        blackFlow.value = prefs.pureBlack
    }

    fun chooseMode(context: Context, mode: ThemeMode) {
        UiPrefs(context).themeMode = mode.key
        modeFlow.value = mode
    }

    fun choosePureBlack(context: Context, on: Boolean) {
        UiPrefs(context).pureBlack = on
        blackFlow.value = on
    }

    fun choose(context: Context, choice: ThemeChoice) {
        UiPrefs(context).theme = choice.key
        flow.value = choice
    }
}

/**
 * A full Material 3 scheme from one line of [PaletteTokens]. The table names the roles the
 * mock-ups use; the few it leaves out are filled with their nearest neighbour, so nothing falls
 * back to Material's own purple.
 */
private fun schemeOf(line: String, dark: Boolean): ColorScheme {
    val c = PaletteTokens.parse(line).mapValues { Color(it.value) }
    fun role(name: String) = c.getValue(name)
    val background = role("background")
    val base = if (dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = role("primary"),
        onPrimary = role("onPrimary"),
        primaryContainer = role("primaryContainer"),
        onPrimaryContainer = role("onPrimaryContainer"),
        inversePrimary = role("inversePrimary"),
        secondary = role("onSurfaceVariant"),
        onSecondary = background,
        secondaryContainer = role("secondaryContainer"),
        onSecondaryContainer = role("onSecondaryContainer"),
        tertiary = role("onTertiaryContainer"),
        onTertiary = role("tertiaryContainer"),
        tertiaryContainer = role("tertiaryContainer"),
        onTertiaryContainer = role("onTertiaryContainer"),
        error = role("error"),
        onError = if (dark) Color(0xFF690005) else Color(0xFFFFFFFF),
        errorContainer = if (dark) Color(0xFF93000A) else Color(0xFFFFDAD6),
        onErrorContainer = if (dark) Color(0xFFFFDAD6) else Color(0xFF410002),
        background = background,
        onBackground = role("onSurface"),
        surface = background,
        onSurface = role("onSurface"),
        surfaceVariant = role("surfaceContainerHigh"),
        onSurfaceVariant = role("onSurfaceVariant"),
        surfaceTint = role("primary"),
        inverseSurface = role("inverseSurface"),
        inverseOnSurface = role("inverseOnSurface"),
        outline = role("outline"),
        outlineVariant = role("outlineVariant"),
        surfaceBright = background,
        surfaceDim = role("surfaceContainerHigh"),
        surfaceContainerLowest = background,
        surfaceContainerLow = role("surfaceContainerLow"),
        surfaceContainer = role("surfaceContainer"),
        surfaceContainerHigh = role("surfaceContainerHigh"),
        surfaceContainerHighest = role("surfaceContainerHigh"),
    )
}

/** The scheme of a built-in theme. [ThemeChoice.Wallpaper] has none here: the phone makes it. */
private fun builtIn(choice: ThemeChoice, dark: Boolean): ColorScheme {
    val palette = PaletteTokens.palettes[choice] ?: PaletteTokens.palettes.getValue(ThemeChoice.Ward)
    return schemeOf(if (dark) palette.dark else palette.light, dark)
}

/** The dark scheme with the page black and the layers above it in dark greys ([BlackRules]); every other role stays. */
fun ColorScheme.pureBlack(): ColorScheme = copy(
    background = Color(BlackRules.SURFACE),
    surface = Color(BlackRules.SURFACE),
    surfaceBright = Color(BlackRules.CONTAINER_HIGHEST),
    surfaceDim = Color(BlackRules.SURFACE),
    surfaceVariant = Color(BlackRules.CONTAINER_HIGH),
    surfaceContainerLowest = Color(BlackRules.CONTAINER_LOWEST),
    surfaceContainerLow = Color(BlackRules.CONTAINER_LOW),
    surfaceContainer = Color(BlackRules.CONTAINER),
    surfaceContainerHigh = Color(BlackRules.CONTAINER_HIGH),
    surfaceContainerHighest = Color(BlackRules.CONTAINER_HIGHEST),
)

/** Whether the app is drawn dark now (the user's mode over the phone's setting); the thread's WebView follows it too. */
val LocalDarkTheme = androidx.compose.runtime.staticCompositionLocalOf { false }

/** The colour a theme is known by in the picker. Null for the wallpaper, which has no fixed one. */
fun seedColor(choice: ThemeChoice): Color? = PaletteTokens.palettes[choice]?.let { Color(it.seed) }

/**
 * Material You. The colours are the user's choice ([ThemeState]): the phone's wallpaper palette
 * (dynamic colour, Android 12 and later) or one of the built-in themes. Light or dark is the user's choice
 * ([ThemeMode], by default the system's), with a black page for dark if asked. Every colour on screen is a role of this scheme, so each theme reaches every screen.
 */
@Composable
fun KwebTheme(content: @Composable () -> Unit) {
    val choice by ThemeState.choice.collectAsStateWithLifecycle()
    val mode by ThemeState.mode.collectAsStateWithLifecycle()
    val black by ThemeState.pureBlack.collectAsStateWithLifecycle()
    val darkTheme = mode.isDark(isSystemInDarkTheme())
    val base = if (choice == ThemeChoice.Wallpaper && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        val context = LocalContext.current
        if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        remember(choice, darkTheme) { builtIn(choice, darkTheme) }
    }
    val colors = if (BlackRules.active(black, mode, isSystemInDarkTheme())) base.pureBlack() else base
    androidx.compose.runtime.CompositionLocalProvider(LocalDarkTheme provides darkTheme) {
        MaterialTheme(colorScheme = colors, content = content)
    }
}
