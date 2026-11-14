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

    val choice: StateFlow<ThemeChoice> get() = flow

    /** Reads the remembered choice. Called before the first frame, so the app never flashes another theme. */
    fun load(context: Context) {
        flow.value = ThemeChoice.fromKey(UiPrefs(context).theme, Build.VERSION.SDK_INT)
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

/** The colour a theme is known by in the picker. Null for the wallpaper, which has no fixed one. */
fun seedColor(choice: ThemeChoice): Color? = PaletteTokens.palettes[choice]?.let { Color(it.seed) }

/**
 * Material You. The colours are the user's choice ([ThemeState]): the phone's wallpaper palette
 * (dynamic colour, Android 12 and later) or one of the built-in themes. Light or dark follows the
 * system. Every colour on screen is a role of this scheme, so each theme reaches every screen.
 */
@Composable
fun KwebTheme(darkTheme: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val choice by ThemeState.choice.collectAsStateWithLifecycle()
    val colors = if (choice == ThemeChoice.Wallpaper && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        val context = LocalContext.current
        if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        remember(choice, darkTheme) { builtIn(choice, darkTheme) }
    }
    MaterialTheme(colorScheme = colors, content = content)
}
