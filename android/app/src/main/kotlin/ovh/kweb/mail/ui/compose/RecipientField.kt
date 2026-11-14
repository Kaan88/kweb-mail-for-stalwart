package ovh.kweb.mail.ui.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Avatar
import ovh.kweb.mail.ui.list.Sender
import uniffi.kweb_core.Address

/** The width the labels of the fields (To, Subject, ...) take, so the texts start at one line. */
val FieldLabelWidth: Dp = 64.dp

/**
 * One recipient field: the label, a chip for each accepted address, and the text being typed.
 * Completion opens as a floating menu under the field that does not take the focus from it.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun RecipientField(
    label: String,
    addresses: List<Address>,
    text: String,
    marked: Boolean,
    suggestions: List<Address>,
    onText: (String) -> Unit,
    onCommit: () -> Unit,
    onRemove: (Address) -> Unit,
    onPick: (Address) -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
    trailing: @Composable (() -> Unit)? = null,
) {
    val focusManager = LocalFocusManager.current
    var focused by remember { mutableStateOf(false) }
    var hadFocus by remember { mutableStateOf(false) }
    var widthPx by remember { mutableIntStateOf(0) }
    val colors = MaterialTheme.colorScheme
    val invalid = stringResource(R.string.compose_invalid_address)

    Box(modifier.fillMaxWidth().onSizeChanged { widthPx = it.width }) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 6.dp)) {
            Row(Modifier.heightIn(min = 36.dp), verticalAlignment = Alignment.Top) {
                Text(
                    label,
                    modifier = Modifier.width(FieldLabelWidth).padding(top = 6.dp),
                    color = colors.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                )
                FlowRow(
                    Modifier.weight(1f),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
                ) {
                    addresses.forEach { Chip(it, onRemove = { _ -> onRemove(it) }) }
                    BasicTextField(
                        value = text,
                        onValueChange = onText,
                        modifier = Modifier
                            .weight(1f)
                            .widthIn(min = 96.dp)
                            .height(32.dp)
                            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                            .onFocusChanged {
                                focused = it.isFocused
                                // Leaving the field finishes what is typed in it.
                                if (hadFocus && !it.isFocused) onCommit()
                                hadFocus = it.isFocused
                            }
                            .onPreviewKeyEvent {
                                if (it.key == Key.Enter && it.type == KeyEventType.KeyDown) {
                                    onCommit()
                                    true
                                } else {
                                    false
                                }
                            }
                            .then(if (marked) Modifier.semantics { error(invalid) } else Modifier),
                        textStyle = MaterialTheme.typography.bodyLarge.copy(color = if (marked) colors.error else colors.onSurface),
                        singleLine = true,
                        cursorBrush = SolidColor(if (marked) colors.error else colors.primary),
                        keyboardOptions = KeyboardOptions(
                            capitalization = KeyboardCapitalization.None,
                            autoCorrectEnabled = false,
                            keyboardType = KeyboardType.Email,
                            imeAction = ImeAction.Next,
                        ),
                        keyboardActions = KeyboardActions(
                            onNext = {
                                // With something typed, Next finishes it and stays; with nothing, it moves on.
                                if (text.isNotBlank()) onCommit() else focusManager.moveFocus(androidx.compose.ui.focus.FocusDirection.Next)
                            },
                        ),
                        decorationBox = { inner -> Box(Modifier.fillMaxWidth().height(32.dp), contentAlignment = Alignment.CenterStart) { inner() } },
                    )
                }
                if (trailing != null) trailing()
            }
            if (marked) {
                Text(
                    invalid,
                    modifier = Modifier.padding(start = FieldLabelWidth, top = 2.dp),
                    color = colors.error,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
        if (focused && suggestions.isNotEmpty()) {
            Popup(popupPositionProvider = BelowAnchor, properties = PopupProperties(focusable = false)) {
                val width = with(LocalDensity.current) { widthPx.toDp() }
                SuggestionMenu(suggestions, onPick, Modifier.width(width - 28.dp))
            }
        }
    }
}

/** The toggle at the end of the To line that shows Cc and Bcc. */
@Composable
fun CcBccToggle(onClick: () -> Unit) {
    TextButton(onClick = onClick) { Text(stringResource(R.string.compose_cc_bcc)) }
}

/** A recipient that was accepted: a pill in the secondary container with a small avatar and a remove button. */
@Composable
private fun Chip(address: Address, onRemove: (Address) -> Unit) {
    val colors = MaterialTheme.colorScheme
    val name = Sender.label(address.name, address.email).orEmpty()
    val removeLabel = stringResource(R.string.compose_remove_recipient, name)
    Row(
        Modifier.height(32.dp).clip(RoundedCornerShape(10.dp)).background(colors.secondaryContainer).padding(start = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Round, in the secondary colour: on a chip that is already the secondary container the usual avatar would not show.
        Box(Modifier.size(24.dp).clip(CircleShape).background(colors.secondary), contentAlignment = Alignment.Center) {
            Text(
                Sender.initial(address.name, address.email),
                color = colors.onSecondary,
                fontWeight = FontWeight.Bold,
                style = MaterialTheme.typography.labelSmall,
            )
        }
        Spacer(Modifier.width(6.dp))
        Text(
            name,
            modifier = Modifier.widthIn(max = 200.dp),
            color = colors.onSecondaryContainer,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.labelLarge,
        )
        Box(
            Modifier.size(32.dp).clickable(onClickLabel = removeLabel) { onRemove(address) },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                Icons.Filled.Close,
                contentDescription = removeLabel,
                modifier = Modifier.size(16.dp),
                tint = colors.onSecondaryContainer,
            )
        }
    }
}

/** The floating completion menu: rounded, raised, a row for each address. */
@Composable
private fun SuggestionMenu(suggestions: List<Address>, onPick: (Address) -> Unit, modifier: Modifier) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(22.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shadowElevation = 6.dp,
    ) {
        Column(Modifier.padding(6.dp)) {
            suggestions.forEach { address ->
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).clickable { onPick(address) }.padding(horizontal = 10.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Avatar(Sender.initial(address.name, address.email), size = 36.dp)
                    Column(Modifier.weight(1f)) {
                        if (address.name.isNotBlank()) {
                            Text(address.name, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        Text(
                            address.email,
                            color = if (address.name.isNotBlank()) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
        }
    }
}

/** Puts a popup just under the box it belongs to, or over it when there is no room below. */
private object BelowAnchor : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val x = (anchorBounds.left + (anchorBounds.width - popupContentSize.width) / 2).coerceIn(0, maxOf(0, windowSize.width - popupContentSize.width))
        val below = anchorBounds.bottom
        val y = if (below + popupContentSize.height <= windowSize.height) below else maxOf(0, anchorBounds.top - popupContentSize.height)
        return IntOffset(x, y)
    }
}
