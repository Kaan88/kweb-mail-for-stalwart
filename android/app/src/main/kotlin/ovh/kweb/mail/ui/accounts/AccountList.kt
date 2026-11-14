package ovh.kweb.mail.ui.accounts

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.setValue
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.getValue
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Avatar
import ovh.kweb.mail.ui.list.Groups

/** What the screens need of the account switcher: the rows, and what a tap on a row or on "Add account" does. */
class AccountsUi(
    val rows: List<AccountRow>,
    val onSwitch: (accountId: String) -> Unit,
    val onAdd: () -> Unit,
)

val LocalAccountsUi = compositionLocalOf { AccountsUi(emptyList(), {}, {}) }

/**
 * The accounts as one group (see [Groups]): each row has the round letter, the login, the server, the unread
 * count of its Inbox, a check on the one that is shown and a warning mark when its sign-in is no longer valid.
 * "Add account" follows the rows. [onPick] gets the id of the row that was tapped (also the one shown).
 */
@Composable
fun AccountList(
    all: List<AccountRow>,
    onPick: (String) -> Unit,
    onAdd: () -> Unit,
    modifier: Modifier = Modifier,
    /** True where the shown account is already above (the drawer's head): only the others are listed. */
    hideActive: Boolean = false,
) {
    val rows = if (hideActive) all.filter { !it.active } else all
    val count = rows.size + 1
    Column(modifier, verticalArrangement = Arrangement.spacedBy(3.dp)) {
        rows.forEachIndexed { index, row -> AccountRowItem(row, Groups.shape(index, count)) { onPick(row.id) } }
        AddRow(Groups.shape(rows.size, count), onAdd)
    }
}

@Composable
private fun AccountRowItem(row: AccountRow, shape: Shape, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(if (row.active) colors.secondaryContainer else colors.surfaceContainerLow)
            .clickable(role = Role.Button, onClick = onClick)
            .heightIn(min = 64.dp)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Avatar(row.initial, strong = row.active)
        Column(Modifier.weight(1f)) {
            Text(
                row.label,
                style = MaterialTheme.typography.titleMedium,
                color = if (row.active) colors.onSecondaryContainer else colors.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                if (row.needsSignIn) stringResource(R.string.accounts_sign_in_needed) else row.host,
                style = MaterialTheme.typography.bodyMedium,
                color = if (row.needsSignIn) colors.error else if (row.active) colors.onSecondaryContainer else colors.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (row.needsSignIn) {
            Icon(Icons.Filled.Warning, contentDescription = stringResource(R.string.accounts_sign_in_needed), tint = colors.error, modifier = Modifier.size(20.dp))
        }
        AccountRows.badge(row.unread)?.let {
            Text(it, style = MaterialTheme.typography.labelLarge, color = if (row.active) colors.onSecondaryContainer else colors.onSurfaceVariant)
        }
        if (row.active) {
            Icon(Icons.Filled.Check, contentDescription = stringResource(R.string.accounts_current), tint = colors.onSecondaryContainer, modifier = Modifier.size(20.dp))
        }
    }
}

@Composable
private fun AddRow(shape: Shape, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .clickable(role = Role.Button, onClick = onClick)
            .heightIn(min = 56.dp)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(Icons.Filled.Add, contentDescription = null, tint = colors.primary, modifier = Modifier.size(40.dp).padding(8.dp))
        Text(stringResource(R.string.accounts_add), style = MaterialTheme.typography.titleMedium, color = colors.primary)
    }
}

/** The sheet the avatar opens: the accounts, to switch between them or to add one. [title] is the headline. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountSheet(title: String, onDismiss: () -> Unit, ui: AccountsUi = LocalAccountsUi.current) {
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = MaterialTheme.colorScheme.surfaceContainer) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 12.dp).padding(bottom = 24.dp)) {
            Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(start = 16.dp, bottom = 12.dp))
            AccountList(
                all = ui.rows,
                onPick = {
                    ui.onSwitch(it)
                    onDismiss()
                },
                onAdd = {
                    ui.onAdd()
                    onDismiss()
                },
            )
        }
    }
}

/**
 * The account's round letter as a button that opens the account sheet. It is the right end of the search
 * field of the four tabs, as on the Inbox, so the account can be switched from each of them.
 */
@Composable
fun AccountAvatarButton(modifier: Modifier = Modifier, ui: AccountsUi = LocalAccountsUi.current) {
    var open by rememberSaveable { mutableStateOf(false) }
    val active = ui.rows.firstOrNull { it.active }
    Box(
        modifier.clip(CircleShape).clickable(onClickLabel = stringResource(R.string.accounts_open_switcher)) { open = true },
    ) { Avatar(active?.initial ?: "?", size = 36.dp) }
    if (open) AccountSheet(title = stringResource(R.string.accounts_title), onDismiss = { open = false }, ui = ui)
}
