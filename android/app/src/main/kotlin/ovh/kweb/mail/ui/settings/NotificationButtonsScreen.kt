package ovh.kweb.mail.ui.settings

import android.app.Application
import androidx.annotation.StringRes
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.MailAction
import ovh.kweb.mail.background.NotificationActionRules
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.ui.accounts.AccountRows
import ovh.kweb.mail.ui.accounts.LocalAccountsUi
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch

/** The name of a notification button, the same text the notification shows. */
@StringRes
internal fun mailActionName(action: MailAction): Int = when (action) {
    MailAction.Archive -> R.string.action_archive
    MailAction.Delete -> R.string.action_delete
    MailAction.MarkRead -> R.string.action_mark_read
    MailAction.Reply -> R.string.action_reply
}

/**
 * The choice of notification buttons of one account. Every change is stored at once, and the notifications of
 * that account that are on screen get the new buttons a moment after the last change ([Notifier.refreshActionsSoon]).
 */
class NotificationButtonsViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val chosenFlow = MutableStateFlow(session.settings.notificationActions)

    val chosen: StateFlow<List<MailAction>> get() = chosenFlow

    fun set(action: MailAction, on: Boolean) = store(NotificationActionRules.set(chosenFlow.value, action, on))

    fun move(action: MailAction, up: Boolean) = store(NotificationActionRules.move(chosenFlow.value, action, up))

    /** Back to the default, stored as "never chose". */
    fun reset() {
        session.settings.resetNotificationActions()
        applied()
    }

    private fun store(next: List<MailAction>) {
        if (next == chosenFlow.value) return
        session.settings.notificationActions = next
        applied()
    }

    private fun applied() {
        chosenFlow.value = session.settings.notificationActions
        Notifier(getApplication()).refreshActionsSoon(session.record)
    }
}

/** "Notification buttons": a preview of the buttons, then one row per action with its switch and its place. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NotificationButtonsScreen(onBack: () -> Unit, vm: NotificationButtonsViewModel = viewModel()) {
    val chosen by vm.chosen.collectAsStateWithLifecycle()
    val rows = NotificationActionRules.rows(chosen)
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.notif_buttons_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            // The choice belongs to the account that is shown; with several, the screen says which.
            val accounts = LocalAccountsUi.current.rows
            accounts.firstOrNull { it.active }?.takeIf { AccountRows.showsAccountNames(accounts.size) }?.let {
                Text(
                    stringResource(R.string.accounts_delivery_for, it.label),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 8.dp),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            SectionTitle(stringResource(R.string.notif_buttons_preview))
            Group { Preview(chosen) }

            SectionTitle(stringResource(R.string.notif_buttons_choose))
            Group {
                rows.forEachIndexed { index, row ->
                    ActionRow(Groups.shape(index, rows.size), row, onSwitch = { vm.set(row.action, it) }, onMove = { vm.move(row.action, it) })
                }
            }
            Text(
                stringResource(R.string.notif_buttons_note),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (chosen != NotificationActionRules.DEFAULT) ResetRow(vm::reset)
        }
    }
}

/** "Use the default", with the default named under it. Shown only while the choice differs from it. */
@Composable
private fun ResetRow(onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val names = NotificationActionRules.DEFAULT.map { stringResource(mailActionName(it)) }.joinToString(", ")
    Group {
        Column(
            Modifier
                .fillMaxWidth()
                .clip(Groups.shape(0, 1))
                .background(colors.surfaceContainerLow)
                .clickable(role = Role.Button, onClick = onClick)
                .heightIn(min = 56.dp)
                .padding(horizontal = 20.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            Text(stringResource(R.string.notif_buttons_reset), color = colors.primary, style = MaterialTheme.typography.bodyLarge)
            Text(names, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

/** A mail notification in small: a made-up sender and subject, and under them the buttons as Android draws them. */
@Composable
private fun Preview(chosen: List<MailAction>) {
    val colors = MaterialTheme.colorScheme
    Column(
        Modifier.fillMaxWidth().clip(Groups.shape(0, 1)).background(colors.surfaceContainerHigh).padding(horizontal = 20.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(stringResource(R.string.notif_buttons_preview_sender), color = colors.onSurface, style = MaterialTheme.typography.titleSmall)
        Text(stringResource(R.string.notif_buttons_preview_subject), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        Row(Modifier.fillMaxWidth().padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(24.dp)) {
            if (chosen.isEmpty()) {
                Text(stringResource(R.string.notif_buttons_preview_none), color = colors.onSurfaceVariant, style = MaterialTheme.typography.labelLarge)
            }
            chosen.forEach {
                Text(
                    stringResource(mailActionName(it)),
                    color = colors.primary,
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/** One action: its icon and name, where it stands, the arrows that move a shown one, and its switch. */
@Composable
private fun ActionRow(shape: Shape, row: NotificationActionRules.Row, onSwitch: (Boolean) -> Unit, onMove: (Boolean) -> Unit) {
    val colors = MaterialTheme.colorScheme
    val name = stringResource(mailActionName(row.action))
    val position = row.position
    val detail = when {
        position != null -> stringResource(R.string.notif_buttons_position, position + 1)
        row.canSwitch -> stringResource(R.string.notif_buttons_off)
        else -> stringResource(R.string.notif_buttons_full)
    }
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .heightIn(min = 64.dp)
            .padding(start = 20.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ActionIcon(row.action)
        Column(Modifier.weight(1f).padding(start = 16.dp)) {
            Text(name, color = colors.onSurface, style = MaterialTheme.typography.bodyLarge)
            Text(detail, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
        if (position != null) {
            IconButton(onClick = { onMove(true) }, enabled = row.canMoveUp) {
                Icon(Icons.Filled.KeyboardArrowUp, contentDescription = stringResource(R.string.notif_buttons_move_up, name))
            }
            IconButton(onClick = { onMove(false) }, enabled = row.canMoveDown) {
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.notif_buttons_move_down, name))
            }
        }
        KwebSwitch(
            checked = position != null,
            onCheckedChange = onSwitch,
            modifier = Modifier.padding(start = 4.dp).semantics { contentDescription = name },
            enabled = position != null || row.canSwitch,
        )
    }
}

@Composable
private fun ActionIcon(action: MailAction) {
    val tint = MaterialTheme.colorScheme.onSurfaceVariant
    when (action) {
        MailAction.Archive -> Icon(painterResource(R.drawable.ic_archive), contentDescription = null, tint = tint)
        MailAction.Delete -> Icon(Icons.Filled.Delete, contentDescription = null, tint = tint)
        MailAction.MarkRead -> Icon(painterResource(R.drawable.ic_drafts), contentDescription = null, tint = tint)
        MailAction.Reply -> Icon(painterResource(R.drawable.ic_reply), contentDescription = null, tint = tint)
    }
}
