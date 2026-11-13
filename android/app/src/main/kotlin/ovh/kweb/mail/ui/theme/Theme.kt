package ovh.kweb.mail.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/** The "Ward" theme from branding/ward. Follows the system dark setting; no dynamic colour. */
private val WardLight = lightColorScheme(
    primary = Color(0xFFC8234A),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFFDE4EA),
    onPrimaryContainer = Color(0xFF8C1733),
    secondaryContainer = Color(0xFFFDE4EA),
    onSecondaryContainer = Color(0xFF8C1733),
    background = Color(0xFFFFFFFF),
    onBackground = Color(0xFF1B1D2A),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF1B1D2A),
    onSurfaceVariant = Color(0xFF666A7B),
    surfaceVariant = Color(0xFFF5F2F4),
    surfaceContainer = Color(0xFFF5F2F4),
    outline = Color(0xFFEBE7EA),
    outlineVariant = Color(0xFFEBE7EA),
)

private val WardDark = darkColorScheme(
    primary = Color(0xFFF0577A),
    onPrimary = Color(0xFF16070C),
    primaryContainer = Color(0xFF4A1424),
    onPrimaryContainer = Color(0xFFFFD5DF),
    secondaryContainer = Color(0xFF4A1424),
    onSecondaryContainer = Color(0xFFFFD5DF),
    background = Color(0xFF15161F),
    onBackground = Color(0xFFECEEF4),
    surface = Color(0xFF15161F),
    onSurface = Color(0xFFECEEF4),
    onSurfaceVariant = Color(0xFF9A9FB2),
    surfaceVariant = Color(0xFF1F212D),
    surfaceContainer = Color(0xFF1F212D),
    outline = Color(0xFF2A2C3A),
    outlineVariant = Color(0xFF2A2C3A),
)

/** The `--ok` and `--warn` pairs of the mock-ups: text colour on its own tinted container. */
@Immutable
class StatusColors(val ok: Color, val okContainer: Color, val warn: Color, val warnContainer: Color)

private val LightStatus = StatusColors(
    ok = Color(0xFF1F7A4D),
    okContainer = Color(0xFFDFF3E8),
    warn = Color(0xFF8A5A00),
    warnContainer = Color(0xFFFBECC8),
)

private val DarkStatus = StatusColors(
    ok = Color(0xFF6FD3A0),
    okContainer = Color(0xFF143325),
    warn = Color(0xFFF0C264),
    warnContainer = Color(0xFF3A2C0A),
)

private val LocalStatusColors = staticCompositionLocalOf { LightStatus }

/** Colours that Material 3 has no slot for. */
object KwebColors {
    val status: StatusColors
        @Composable @ReadOnlyComposable get() = LocalStatusColors.current
}

@Composable
fun KwebTheme(darkTheme: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalStatusColors provides if (darkTheme) DarkStatus else LightStatus) {
        MaterialTheme(colorScheme = if (darkTheme) WardDark else WardLight, content = content)
    }
}
