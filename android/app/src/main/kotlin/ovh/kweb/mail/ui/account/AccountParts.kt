package ovh.kweb.mail.ui.account

import android.app.Activity
import android.content.ClipData
import android.content.ClipDescription
import android.content.ContextWrapper
import android.os.Build
import android.os.PersistableBundle
import android.view.WindowManager
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.intl.Locale as ComposeLocale
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.input.KeyboardCapitalization
import kotlinx.coroutines.launch
import ovh.kweb.mail.R

/** Keeps the window out of screenshots and the recent-apps picture while [active] (a password, a code or a secret is on screen). */
@Composable
fun SecureWindow(active: Boolean) {
    val context = LocalContext.current
    DisposableEffect(context, active) {
        val window = generateSequence(context) { (it as? ContextWrapper)?.baseContext }.filterIsInstance<Activity>().firstOrNull()?.window
        val set = active && window != null && (window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE) == 0
        if (set) window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { if (set) window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
}

/**
 * Puts a text on the clipboard. [sensitive] marks it so the keyboard and the system's preview do not show it.
 * [onCopied] runs afterwards; from Android 13 the system says "Copied" itself, so the caller shows its own line only before.
 */
@Composable
fun rememberCopier(sensitive: Boolean, onCopied: () -> Unit): (String) -> Unit {
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    return remember(clipboard, sensitive) {
        { text ->
            scope.launch {
                val clip = ClipData.newPlainText("", text)
                if (sensitive && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    clip.description.extras = PersistableBundle().apply { putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true) }
                }
                clipboard.setClipEntry(ClipEntry(clip))
                onCopied()
            }
        }
    }
}

/** The system already confirms a copy from Android 13. */
val systemConfirmsCopy: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU

@Composable
fun AccountSectionTitle(text: String) {
    Text(
        text.uppercase(ComposeLocale.current.platformLocale),
        modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

/** The rows of one group, a small gap apart. */
@Composable
fun AccountGroup(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Column(modifier.padding(horizontal = 12.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) { content() }
}

/** One row of a group: [leading] and [trailing] around a title and a quiet line. [onClick] null: not a button. */
@Composable
fun AccountRow(
    shape: Shape,
    title: String,
    modifier: Modifier = Modifier,
    detail: String? = null,
    leading: (@Composable () -> Unit)? = null,
    titleColor: Color = Color.Unspecified,
    quiet: Boolean = false,
    onClick: (() -> Unit)? = null,
    titleContent: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val colors = MaterialTheme.colorScheme
    Row(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
            .heightIn(min = 56.dp)
            .padding(start = 20.dp, end = if (trailing != null) 8.dp else 20.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        leading?.invoke()
        Column(Modifier.weight(1f)) {
            if (titleContent != null) {
                titleContent()
            } else {
                Text(
                    title,
                    color = if (quiet) colors.onSurfaceVariant else if (titleColor == Color.Unspecified) colors.onSurface else titleColor,
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
            if (detail != null) Text(detail, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
        trailing?.invoke()
    }
}

/** A small label on a row ("This app"). */
@Composable
fun Pill(text: String) {
    Text(
        text,
        modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(MaterialTheme.colorScheme.secondaryContainer).padding(horizontal = 8.dp, vertical = 2.dp),
        color = MaterialTheme.colorScheme.onSecondaryContainer,
        style = MaterialTheme.typography.labelSmall,
    )
}

/** What a screen says when it cannot show its list, with a way to try again. */
@Composable
fun PageMessage(text: String, retry: () -> Unit) {
    Column(Modifier.fillMaxSize().padding(32.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
        Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
        TextButton(onClick = retry) { Text(stringResource(R.string.retry)) }
    }
}

/** A password: hidden, with the password keyboard and no suggestions or auto-correction. Never saved with the screen's state. */
@Composable
fun PasswordField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    isError: Boolean = false,
    supporting: String? = null,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier.fillMaxWidth(),
        label = { Text(label) },
        singleLine = true,
        enabled = enabled,
        isError = isError,
        supportingText = supporting?.let { { Text(it) } },
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = KeyboardType.Password),
    )
}

/** The hidden-text transformation is not wanted for a code that is typed once and shown in the clear. */
@Composable
fun CodeField(value: String, onValueChange: (String) -> Unit, label: String, modifier: Modifier = Modifier, enabled: Boolean = true) {
    OutlinedTextField(
        value = value,
        onValueChange = { onValueChange(it.filter { c -> c.isDigit() || c.isWhitespace() }.take(7)) },
        modifier = modifier.fillMaxWidth(),
        label = { Text(label) },
        singleLine = true,
        enabled = enabled,
        visualTransformation = VisualTransformation.None,
        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = KeyboardType.NumberPassword),
    )
}

