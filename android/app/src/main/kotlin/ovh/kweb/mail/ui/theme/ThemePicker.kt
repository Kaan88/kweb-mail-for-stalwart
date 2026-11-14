package ovh.kweb.mail.ui.theme

import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import ovh.kweb.mail.R

/** The name of a theme in the picker. */
fun themeName(choice: ThemeChoice): Int = when (choice) {
    ThemeChoice.Wallpaper -> R.string.theme_wallpaper
    ThemeChoice.Ward -> R.string.theme_ward
    ThemeChoice.Lavender -> R.string.theme_lavender
    ThemeChoice.Moss -> R.string.theme_moss
    ThemeChoice.Peach -> R.string.theme_peach
}

/** Choosing a theme: it is applied at once, behind the dialog, so the choice can be judged before closing. */
@Composable
fun ThemePickerDialog(onDismiss: () -> Unit) {
    val context = LocalContext.current
    val current by ThemeState.choice.collectAsStateWithLifecycle()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.theme_title)) },
        text = {
            Column(Modifier.selectableGroup()) {
                ThemeChoice.offered(Build.VERSION.SDK_INT).forEach { choice ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .selectable(
                                selected = choice == current,
                                onClick = { ThemeState.choose(context, choice) },
                                role = Role.RadioButton,
                            )
                            .heightIn(min = 56.dp)
                            .padding(vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        Swatch(choice)
                        Column(Modifier.weight(1f)) {
                            Text(stringResource(themeName(choice)), style = MaterialTheme.typography.bodyLarge)
                            if (choice == ThemeChoice.Wallpaper) {
                                Text(
                                    stringResource(R.string.theme_wallpaper_hint),
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        }
                        // The row is the control; the button only shows the state.
                        RadioButton(selected = choice == current, onClick = null)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.theme_done)) } },
    )
}

/** The colour a theme is known by; the wallpaper theme shows the phone's own primary colour. */
@Composable
private fun Swatch(choice: ThemeChoice) {
    val color = seedColor(choice) ?: wallpaperPrimary()
    Box(
        Modifier
            .size(28.dp)
            .clip(CircleShape)
            .background(color)
            .border(1.dp, MaterialTheme.colorScheme.outlineVariant, CircleShape),
    )
}

@Composable
private fun wallpaperPrimary() =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        androidx.compose.material3.dynamicLightColorScheme(LocalContext.current).primary
    } else {
        MaterialTheme.colorScheme.primary
    }
