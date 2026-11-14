package ovh.kweb.mail.ui.enterprise

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.Tenant

/** The tenants of the server, each with a bar for how much of its storage limit is used. */
@Composable
fun TenantsScreen(onBack: () -> Unit, vm: TenantsViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start() }

    EnterpriseScaffold(stringResource(R.string.ent_tenants_title), onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            val tenants = state.value
            val error = state.error
            if (error != null && tenants != null) NoticeBar(errorText(error), error = true, action = stringResource(R.string.ent_dismiss) to c::dismissError)
            when {
                tenants == null && state.phase == Phase.FAILED -> FailedBox(error, R.string.ent_tenants_failed, c::reload)
                tenants == null -> LoadingBox()
                tenants.isEmpty() -> EmptyState(R.drawable.ic_group, stringResource(R.string.ent_tenants_none), stringResource(R.string.ent_tenants_none_body))
                else -> Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
                    GroupColumn {
                        tenants.forEachIndexed { index, tenant -> TenantRow(Groups.shape(index, tenants.size), tenant) }
                    }
                }
            }
        }
    }
}

@Composable
private fun TenantRow(shape: androidx.compose.ui.graphics.Shape, tenant: Tenant) {
    val fraction = SizeText.fraction(tenant.used, tenant.quota)
    val colors = MaterialTheme.colorScheme
    GroupRow(shape) {
        Column(Modifier.weight(1f).padding(vertical = 6.dp)) {
            Text(tenant.name, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
            val used = tenant.used?.let { SizeText.bytes(it) }
            val quota = tenant.quota?.let { SizeText.bytes(it) }
            Text(
                when {
                    used != null && quota != null -> stringResource(R.string.ent_tenant_of, used, quota)
                    used != null -> stringResource(R.string.ent_tenant_no_limit, used)
                    quota != null -> stringResource(R.string.ent_tenant_limit_only, quota)
                    else -> stringResource(R.string.ent_none)
                },
                style = MaterialTheme.typography.bodySmall,
                color = colors.onSurfaceVariant,
            )
            if (fraction != null) {
                LinearProgressIndicator(
                    progress = { fraction },
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                    color = if (SizeText.isNearlyFull(fraction)) colors.error else colors.primary,
                    trackColor = colors.surfaceContainerHighest,
                )
            }
        }
    }
}
