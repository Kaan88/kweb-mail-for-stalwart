package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.EmptyNote
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.ServerAccount

/** Server accounts: everyone who has a mailbox on the server, with a search field and a plus for a new account. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountsScreen(onBack: () -> Unit, onOpen: (AdminScreen) -> Unit, vm: AdminAccountsViewModel = viewModel()) {
    val c = vm.accounts
    val list by c.list.state.collectAsStateWithLifecycle()
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    AdminScaffold(
        title = stringResource(R.string.admin_area_accounts),
        onBack = onBack,
        actions = {
            IconButton(onClick = { onOpen(AdminScreen.NewAccount) }) { Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.admin_new_account)) }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            AdminField(
                value = state.text,
                onValueChange = c::setText,
                label = state.knownTotal?.let { pluralStringResource(R.plurals.admin_search_accounts, it, it) } ?: stringResource(R.string.admin_search_accounts_plain),
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { c.list.reload(quiet = true) }),
            )
            Notices(list.error, null, c.list::dismissError) {}
            when (list.phase) {
                ListPhase.LOADING -> LoadingBox()
                ListPhase.FAILED -> FailureBox(stringResource(R.string.admin_accounts_load_failed), onRetry = { c.list.reload(quiet = false) })
                ListPhase.READY -> PullToRefreshBox(isRefreshing = list.refreshing, onRefresh = c::refresh, modifier = Modifier.fillMaxSize()) {
                    if (list.items.isEmpty()) {
                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.Center) {
                            EmptyNote(R.drawable.ic_person, stringResource(R.string.admin_accounts_none), modifier = Modifier.fillMaxWidth())
                        }
                    } else {
                        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            itemsIndexed(list.items, key = { _, a -> a.id }) { index, account ->
                                AccountRow(Groups.shape(index, list.items.size), account) { onOpen(AdminScreen.Account(account.id)) }
                            }
                            if (list.more || list.loadingMore) {
                                item(key = "more") {
                                    TextButton(onClick = c.list::loadMore, enabled = !list.loadingMore, modifier = Modifier.fillMaxWidth()) {
                                        Text(stringResource(if (list.loadingMore) R.string.admin_loading else R.string.admin_load_more))
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun AccountRow(shape: androidx.compose.ui.graphics.Shape, account: ServerAccount, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        RoundMark(AdminRules.initial(account.name))
        Column(Modifier.weight(1f)) {
            Text(account.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(AdminRules.secondLine(account), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
            for (pill in AdminRules.pills(account)) Pill(pillWord(pill), error = pill == AccountPill.Suspended)
        }
    }
}
