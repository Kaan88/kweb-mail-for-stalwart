package ovh.kweb.mail.ui.admin

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.stringResource
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Icon
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R

/** The quiet line of the row: nothing while it is read, otherwise what the account's admin area is. */
internal fun settingsLine(state: AccessState): Int? = when {
    state.access == null -> null
    state.kind == AccessKind.Open -> R.string.admin_row_open
    state.kind == AccessKind.Refused -> R.string.admin_row_refused
    else -> R.string.admin_row_locked
}

/**
 * The "Server administration" row of Settings. It is there for every account (the app cannot know who is an
 * administrator); its second line says whether the admin area is locked or open. It asks the core, which does not
 * go to the server while no key is stored.
 */
@Composable
fun AdminSettingsRow(shape: Shape, onOpen: () -> Unit, modifier: Modifier = Modifier, vm: AdminRowViewModel = viewModel()) {
    val state by vm.access.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        vm.access.load()
        onPauseOrDispose { }
    }
    AdminRow(
        shape,
        stringResource(R.string.admin_title),
        modifier = modifier,
        detail = settingsLine(state)?.let { stringResource(it) },
        leading = { Icon(Icons.Filled.Lock, contentDescription = null) },
        onClick = onOpen,
    )
}
