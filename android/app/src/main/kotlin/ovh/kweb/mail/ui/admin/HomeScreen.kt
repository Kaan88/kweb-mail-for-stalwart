package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.AccountBox
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.DateRange
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.AdminSummary

/** The icon of an area: a vector, or the app's own drawable. */
private fun iconOf(area: AdminArea): ImageVector? = when (area) {
    AdminArea.Queue -> Icons.AutoMirrored.Filled.Send
    AdminArea.Accounts -> Icons.Filled.Person
    AdminArea.Blocked -> null
    AdminArea.Logs -> Icons.AutoMirrored.Filled.List
    AdminArea.Maintenance -> Icons.Filled.Build
    AdminArea.Dashboard -> Icons.Filled.Home
    AdminArea.History -> Icons.Filled.DateRange
    AdminArea.Recovery -> Icons.Filled.Refresh
    AdminArea.Alerts -> Icons.Filled.Notifications
    AdminArea.Tenants -> Icons.Filled.AccountBox
}

private fun screenOf(area: AdminArea): AdminScreen = when (area) {
    AdminArea.Queue -> AdminScreen.Queue
    AdminArea.Accounts -> AdminScreen.Accounts
    AdminArea.Blocked -> AdminScreen.Blocked
    AdminArea.Logs -> AdminScreen.Logs
    AdminArea.Maintenance -> AdminScreen.Maintenance
    else -> AdminScreen.Enterprise(area)
}

private fun figureOf(area: AdminArea, summary: AdminSummary?): String? = when (area) {
    AdminArea.Queue -> summary?.queueWaiting?.toString()
    AdminArea.Accounts -> summary?.accounts?.toString()
    AdminArea.Blocked -> summary?.blocked?.toString()
    else -> null
}

/**
 * The start of the admin area: a plain list of what can be done, with a live figure where there is one. Without a key
 * it shows the access screen's content instead (the key is pasted here).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AdminHomeScreen(onBack: () -> Unit, onOpen: (AdminScreen) -> Unit, vm: AdminHomeViewModel = viewModel()) {
    val home = vm.home
    val state by home.state.collectAsStateWithLifecycle()
    val access by home.access.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        home.refresh()
        onPauseOrDispose { }
    }
    val open = access.kind == AccessKind.Open
    AdminScaffold(
        title = stringResource(if (open) R.string.admin_home_title else R.string.admin_access_title),
        onBack = onBack,
        secure = !open,
        actions = {
            val granted = access.access
            if (open && granted != null) {
                Pill(editionName(granted.edition), modifier = Modifier.padding(end = 4.dp))
                IconButton(onClick = { onOpen(AdminScreen.Access) }) {
                    Icon(Icons.Filled.Lock, contentDescription = stringResource(R.string.admin_access_title))
                }
            }
        },
    ) { padding ->
        val granted = access.access
        if (!open || granted == null) {
            Column(Modifier.padding(padding).fillMaxSize()) { AccessContent(home.access, access) }
        } else {
            PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = home::refresh, modifier = Modifier.padding(padding).fillMaxSize()) {
                HomeList(granted, state, onOpen, home)
            }
        }
    }
}

@Composable
private fun HomeList(access: uniffi.kweb_core.AdminAccess, state: HomeState, onOpen: (AdminScreen) -> Unit, home: HomeController) {
    val areas = AdminRules.homeAreas(access)
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        Notices(state.error, null, home::dismissError) {}
        if (areas.upper.isEmpty() && areas.lower.isEmpty()) Note(stringResource(R.string.admin_nothing_allowed))
        AdminGroup {
            areas.upper.forEachIndexed { index, area ->
                AreaRow(Groups.shape(index, areas.upper.size), area, figureOf(area, state.summary), dimmed = false) { onOpen(screenOf(area)) }
            }
        }
        if (areas.lower.isNotEmpty()) {
            if (areas.lowerDimmed) {
                Note(stringResource(R.string.admin_need_licence), Modifier.padding(top = 12.dp))
            } else {
                SectionLabel(stringResource(R.string.admin_enterprise_section))
            }
            AdminGroup {
                areas.lower.forEachIndexed { index, area ->
                    AreaRow(Groups.shape(index, areas.lower.size), area, null, dimmed = areas.lowerDimmed) { onOpen(screenOf(area)) }
                }
            }
        }
    }
}

@Composable
private fun AreaRow(shape: androidx.compose.ui.graphics.Shape, area: AdminArea, figure: String?, dimmed: Boolean, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val vector = iconOf(area)
    AdminRow(
        shape,
        areaTitle(area),
        dimmed = dimmed,
        onClick = onClick,
        leading = {
            if (vector != null) {
                Icon(vector, contentDescription = null, tint = colors.onSurfaceVariant)
            } else {
                Icon(painterResource(R.drawable.ic_block), contentDescription = null, tint = colors.onSurfaceVariant)
            }
        },
        trailing = {
            if (figure != null) Text(figure, color = colors.onSurfaceVariant, style = MaterialTheme.typography.titleMedium)
            if (!dimmed) Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null, tint = colors.onSurfaceVariant, modifier = Modifier.size(24.dp))
        },
    )
}
