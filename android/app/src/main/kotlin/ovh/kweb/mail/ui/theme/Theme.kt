package ovh.kweb.mail.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
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
    surfaceVariant = Color(0xFF1F212D),
    surfaceContainer = Color(0xFF1F212D),
    outline = Color(0xFF2A2C3A),
    outlineVariant = Color(0xFF2A2C3A),
)

@Composable
fun KwebTheme(darkTheme: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (darkTheme) WardDark else WardLight, content = content)
}
