package ovh.kweb.mail.ui.sharing

import ovh.kweb.mail.ui.accounts.LocalAccountSession
import android.app.Application
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.shared.CollectSharedEvents
import ovh.kweb.mail.ui.shared.GroupHeading
import ovh.kweb.mail.ui.shared.Loading
import ovh.kweb.mail.ui.shared.Note
import ovh.kweb.mail.ui.shared.SharedCenteredBox
import ovh.kweb.mail.ui.shared.SharedTopBar
import ovh.kweb.mail.ui.shared.text
import uniffi.kweb_core.Person
import uniffi.kweb_core.Share
import uniffi.kweb_core.ShareLevel

@Composable
internal fun levelLabel(level: ShareLevel): String =
    stringResource(if (level == ShareLevel.WRITE) R.string.share_level_write else R.string.share_level_read)

/**
 * Whom one thing of the user's own (a mail folder, a calendar, an address book, a file folder) is shared with:
 * each person with a level that can be changed or taken back (after a question), and a lookup to add more. When the
 * server does not let the app look people up, its words stand where the lookup would be and the shares can still be changed.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareScreen(route: Route.Share, onBack: () -> Unit) {
    val app = LocalContext.current.applicationContext as Application
    val accountSession = LocalAccountSession.current
    val vm: ShareViewModel = viewModel(factory = viewModelFactory { initializer { ShareViewModel(app, accountSession, route.kind, route.id) } })
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    CollectSharedEvents(c.events, snackbar, onGone = {})
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }
    val shares = state.shares

    Scaffold(
        topBar = { SharedTopBar(stringResource(R.string.share_title, route.name), onBack) },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (state.busyPersonId != null) LinearProgressIndicator(Modifier.fillMaxWidth())
            when {
                shares == null && state.error != null -> SharedCenteredBox {
                    Note(stringResource(R.string.share_failed) + "\n" + state.error!!.text(), onRetry = c::refresh)
                }
                shares == null -> SharedCenteredBox { Loading() }
                else -> ShareBody(state, c, shares)
            }
        }
    }

    state.confirmRemove?.let { person ->
        AlertDialog(
            onDismissRequest = c::cancelRemove,
            title = { Text(stringResource(R.string.share_remove_title, ShareRules.label(person))) },
            text = { Text(stringResource(R.string.share_remove_text)) },
            confirmButton = {
                TextButton(onClick = c::confirmRemove) { Text(stringResource(R.string.share_stop), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = c::cancelRemove) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun ShareBody(state: ShareState, c: ShareController, shares: List<Share>) {
    val candidates = ShareRules.candidates(state.people, shares)
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "with") { GroupHeading(stringResource(R.string.share_with_heading)) }
        if (shares.isEmpty()) {
            item(key = "nobody") {
                Text(
                    stringResource(R.string.share_nobody),
                    modifier = Modifier.padding(horizontal = 28.dp, vertical = 8.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        itemsIndexed(shares, key = { _, share -> "s/${share.person.id}" }) { index, share ->
            ShareRow(Groups.shape(index, shares.size), share, enabled = state.busyPersonId == null, onLevel = { c.changeLevel(share.person, it) }, onStop = { c.askRemove(share.person) })
        }
        item(key = "add") { GroupHeading(stringResource(R.string.share_add_heading)) }
        item(key = "level") { LevelChoice(state.newLevel, c::setNewLevel) }
        val refused = state.lookupRefused
        if (refused != null) {
            item(key = "refused") {
                Text(
                    refused.text(),
                    modifier = Modifier.padding(horizontal = 28.dp, vertical = 8.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            item(key = "search") { SearchField(state.query, c::onQueryChange) }
            item(key = "status") { SearchStatus(state, candidates.isEmpty()) }
            itemsIndexed(candidates, key = { _, person -> "p/${person.id}" }) { index, person ->
                PersonRow(Groups.shape(index, candidates.size), person, enabled = state.busyPersonId == null, onClick = { c.add(person) })
            }
        }
    }
}

@Composable
private fun ShareRow(shape: androidx.compose.ui.graphics.Shape, share: Share, enabled: Boolean, onLevel: (ShareLevel) -> Unit, onStop: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    var open by remember { mutableStateOf(false) }
    val second = ShareRules.secondLine(share.person)
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 1.5.dp).clip(shape).background(colors.surfaceContainerLow)
            .heightIn(min = 64.dp).padding(start = 20.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(ShareRules.label(share.person), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (second != null) {
                Text(second, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        Box {
            TextButton(onClick = { open = true }, enabled = enabled) { Text(levelLabel(share.level)) }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                for (level in ShareRules.LEVELS) {
                    DropdownMenuItem(text = { Text(levelLabel(level)) }, onClick = { open = false; onLevel(level) })
                }
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.share_stop), color = colors.error) },
                    onClick = { open = false; onStop() },
                )
            }
        }
    }
}

@Composable
private fun LevelChoice(selected: ShareLevel, onSelect: (ShareLevel) -> Unit) {
    Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
        Text(stringResource(R.string.share_new_level), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(top = 4.dp)) {
            ShareRules.LEVELS.forEachIndexed { index, level ->
                SegmentedButton(
                    selected = level == selected,
                    onClick = { onSelect(level) },
                    shape = SegmentedButtonDefaults.itemShape(index, ShareRules.LEVELS.size),
                ) { Text(levelLabel(level)) }
            }
        }
    }
}

@Composable
private fun SearchField(query: String, onChange: (String) -> Unit) {
    // The text is kept here so the cursor does not jump; the controller only empties it after a person was added.
    var text by rememberSaveable { mutableStateOf(query) }
    if (query.isEmpty() && text.isNotEmpty()) text = ""
    OutlinedTextField(
        value = text,
        onValueChange = {
            text = it
            onChange(it)
        },
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        label = { Text(stringResource(R.string.share_search_label)) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        trailingIcon = {
            if (text.isNotEmpty()) {
                IconButton(onClick = { text = ""; onChange("") }) {
                    Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.share_clear_search))
                }
            }
        },
    )
}

@Composable
private fun SearchStatus(state: ShareState, nothingToOffer: Boolean) {
    val line = when {
        state.searchError != null -> stringResource(R.string.share_search_failed) + "\n" + state.searchError.text()
        !ShareRules.canSearch(state.query) -> stringResource(R.string.share_search_hint)
        state.searching -> stringResource(R.string.share_searching)
        nothingToOffer && state.answeredQuery.isNotEmpty() -> stringResource(R.string.share_no_people)
        else -> null
    }
    if (line != null) {
        Text(line, modifier = Modifier.padding(horizontal = 28.dp, vertical = 4.dp), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun PersonRow(shape: androidx.compose.ui.graphics.Shape, person: Person, enabled: Boolean, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val second = ShareRules.secondLine(person)
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 1.5.dp).clip(shape).background(colors.surfaceContainerLow)
            .clickable(enabled = enabled, role = Role.Button, onClickLabel = stringResource(R.string.share_add_person, ShareRules.label(person)), onClick = onClick)
            .heightIn(min = 56.dp).padding(horizontal = 20.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Text(ShareRules.label(person), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (second != null) {
            Text(second, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}
