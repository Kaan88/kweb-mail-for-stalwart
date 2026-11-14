package ovh.kweb.mail.ui.contacts

import androidx.compose.foundation.background
import androidx.compose.ui.focus.focusRequester
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.compose.ui.focus.FocusManager
import androidx.compose.ui.platform.LocalFocusManager
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.accounts.AccountAvatarButton
import ovh.kweb.mail.ui.sharing.PickKind
import ovh.kweb.mail.ui.sharing.SharePickerButton
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.ContactRow

/**
 * The contacts, as the Contacts figure: a search pill, then the contacts under large letters, a round "+"
 * button, and the bottom bar. [onOpenContact] opens one; [onAddContact] opens the empty form.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContactsScreen(
    onOpenContact: (contactId: String) -> Unit,
    onAddContact: () -> Unit,
    bottomBar: @Composable () -> Unit,
    appSnackbar: SnackbarHostState,
    vm: ContactsViewModel = viewModel(),
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val locale = rememberLocale()
    val entries = remember(state.rows, locale) { ContactRules.group(state.rows, locale) }

    // Every time the screen comes up, also from the background, the list is asked for again.
    LifecycleResumeEffect(vm) {
        vm.load()
        onPauseOrDispose { }
    }

    Scaffold(
        topBar = {
            Column(Modifier.background(MaterialTheme.colorScheme.background).statusBarsPadding()) {
                SearchPill(initial = state.query, onChange = vm::onQueryChange, trailing = {
                    SharePickerButton(PickKind.ADDRESS_BOOK)
                    AccountAvatarButton()
                })
            }
        },
        bottomBar = bottomBar,
        snackbarHost = { SnackbarHost(appSnackbar) },
        floatingActionButton = {
            FloatingActionButton(
                onClick = onAddContact,
                shape = RoundedCornerShape(20.dp),
                containerColor = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            ) { Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.contacts_add)) }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (state.fromCache) NoticeBar(stringResource(R.string.contacts_offline), error = false)
            if (state.failed && state.loaded) {
                NoticeBar(stringResource(R.string.contacts_update_failed), error = true, action = stringResource(R.string.retry) to vm::refresh)
            }
            PullToRefreshBox(
                isRefreshing = state.refreshing,
                onRefresh = vm::refresh,
                modifier = Modifier.weight(1f).fillMaxWidth(),
            ) {
                LazyColumn(Modifier.fillMaxSize(), state = vm.listState, contentPadding = PaddingValues(bottom = 96.dp)) {
                    when {
                        !state.loaded && !state.failed -> item(key = "loading") { Centered { CircularProgressIndicator() } }
                        !state.loaded -> item(key = "failed") {
                            Centered {
                                Text(
                                    stringResource(R.string.contacts_load_failed),
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    textAlign = TextAlign.Center,
                                )
                                TextButton(onClick = vm::refresh) { Text(stringResource(R.string.retry)) }
                            }
                        }
                        entries.isEmpty() -> item(key = "empty") {
                            Centered {
                                Text(
                                    stringResource(if (state.answeredQuery.isBlank()) R.string.contacts_empty else R.string.contacts_nothing_found),
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    textAlign = TextAlign.Center,
                                )
                            }
                        }
                        else -> items(entries, key = { entry -> entryKey(entry) }) { entry ->
                            when (entry) {
                                is ContactRules.Entry.Heading -> LetterHeading(entry.letter)
                                is ContactRules.Entry.Item -> ContactItem(entry, locale, onClick = { onOpenContact(entry.row.id) })
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun entryKey(entry: ContactRules.Entry): String = when (entry) {
    is ContactRules.Entry.Heading -> "letter/${entry.letter}"
    is ContactRules.Entry.Item -> "contact/${entry.row.id}"
}

/** Content in the middle of the list area, with room to pull down on. */
@Composable
private fun LazyItemScope.Centered(content: @Composable () -> Unit) {
    Column(
        Modifier.fillParentMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) { content() }
}

/**
 * The 52 dp pill of the figure. The text is kept here, not read back from the view model, so that the cursor
 * does not jump while the view model answers; the view model's query only fills it when the screen comes up.
 */
@Composable
internal fun SearchPill(
    initial: String,
    onChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    /** At the right end, after the text (a menu, the account). */
    trailing: (@Composable () -> Unit)? = null,
    /** Replaces the search icon at the left (a back arrow). */
    leading: (@Composable () -> Unit)? = null,
    /** Opens the keyboard when the field comes up. */
    autoFocus: Boolean = false,
    /** When given, the field is not typed into: a tap anywhere on it calls this (it opens the real search). */
    onTap: (() -> Unit)? = null,
    label: String = stringResource(R.string.contacts_search),
) {
    val colors = MaterialTheme.colorScheme
    var text by remember { mutableStateOf(initial) }
    val focus: FocusManager = LocalFocusManager.current
    val focusRequester = remember { androidx.compose.ui.focus.FocusRequester() }
    androidx.compose.runtime.LaunchedEffect(autoFocus) { if (autoFocus) runCatching { focusRequester.requestFocus() } }
    // The same field as the Inbox's: 52 dp, raised surface, round, with the account at its right end.
    Row(
        modifier
            .fillMaxWidth()
            .padding(start = 14.dp, end = 14.dp, top = 8.dp, bottom = 8.dp)
            .height(52.dp)
            .clip(CircleShape)
            .background(colors.surfaceContainerHigh)
            .then(if (onTap != null) Modifier.clickable(onClick = onTap) else Modifier)
            .padding(start = if (leading != null) 4.dp else 16.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(if (leading != null) 4.dp else 12.dp),
    ) {
        if (leading != null) leading() else Icon(Icons.Filled.Search, contentDescription = null, tint = colors.onSurfaceVariant)
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (text.isEmpty()) Text(label, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
            if (onTap == null) BasicTextField(
                value = text,
                onValueChange = {
                    text = it
                    onChange(it)
                },
                singleLine = true,
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.onSurface),
                cursorBrush = SolidColor(colors.primary),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { focus.clearFocus() }),
                modifier = Modifier.fillMaxWidth().focusRequester(focusRequester).semantics { contentDescription = label },
            )
        }
        if (text.isNotEmpty() && onTap == null) {
            IconButton(onClick = {
                text = ""
                onChange("")
            }) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.contacts_clear_search)) }
        }
        if (trailing != null) trailing()
    }
}

@Composable
internal fun LetterHeading(letter: String) {
    Text(
        letter,
        modifier = Modifier.fillMaxWidth().padding(start = 24.dp, end = 20.dp, top = 14.dp, bottom = 4.dp).semantics { heading() },
        style = MaterialTheme.typography.titleLarge,
        fontWeight = FontWeight.SemiBold,
        color = MaterialTheme.colorScheme.primary,
    )
}

/** One contact: a tonal round avatar with the initial, the name, and the quiet line. It is a row of its letter's group. */
@Composable
internal fun ContactItem(item: ContactRules.Entry.Item, locale: java.util.Locale, onClick: () -> Unit) {
    val row: ContactRow = item.row
    val subtitle = remember(row) { ContactRules.subtitle(row) }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp)
            .padding(top = if (item.indexInGroup == 0) 0.dp else 3.dp)
            .clip(Groups.shape(item.indexInGroup, item.groupSize))
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        ContactAvatar(ContactRules.initial(row.name, locale), size = 44)
        Column(Modifier.weight(1f)) {
            Text(row.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (subtitle.isNotEmpty()) {
                Text(
                    subtitle,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/** The tonal round avatar of the figure: the secondary container with the initial on it. */
@Composable
internal fun ContactAvatar(initial: String, size: Int) {
    val colors = MaterialTheme.colorScheme
    Box(
        Modifier.size(size.dp).clip(CircleShape).background(colors.secondaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            initial,
            color = colors.onSecondaryContainer,
            fontWeight = FontWeight.SemiBold,
            style = if (size >= 80) MaterialTheme.typography.displaySmall else MaterialTheme.typography.titleMedium,
        )
    }
}
