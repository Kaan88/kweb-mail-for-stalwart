package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups

/** The form for a new account: name, domain, description and a password typed twice. */
@Composable
fun NewAccountScreen(onBack: () -> Unit, vm: AdminNewAccountViewModel = viewModel()) {
    val c = vm.form
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.load() }
    LaunchedEffect(state.created) { if (state.created != null) onBack() }
    AdminScaffold(title = stringResource(R.string.admin_new_account), onBack = onBack, secure = true) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error, null, c::dismissError) {}
            when (state.phase) {
                NewAccountPhase.LOADING -> LoadingBox()
                NewAccountPhase.FAILED -> FailureBox(stringResource(R.string.admin_domains_failed), onRetry = c::load)
                NewAccountPhase.READY -> Form(state, c)
            }
        }
    }
}

@Composable
private fun Form(state: NewAccountState, c: NewAccountController) {
    var touched by rememberSaveable { mutableStateOf(false) }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 12.dp).padding(bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Note(stringResource(R.string.admin_new_account_note), Modifier.padding(horizontal = 16.dp))
        AdminField(
            state.name,
            { touched = true; c.typeName(it) },
            stringResource(R.string.admin_name_field),
            isError = touched && state.nameProblem != null,
            supporting = if (touched) nameHint(state.nameProblem) else null,
        )
        DomainPicker(state, c)
        AdminField(state.description, c::typeDescription, stringResource(R.string.admin_description))
        AdminField(state.password, { touched = true; c.typePassword(it) }, stringResource(R.string.admin_password), secret = true, isError = touched && state.passwordProblem == PasswordProblem.TooShort)
        AdminField(
            state.repeat,
            { touched = true; c.typeRepeat(it) },
            stringResource(R.string.admin_repeat_password),
            secret = true,
            isError = state.passwordProblem == PasswordProblem.Different && state.repeat.isNotEmpty(),
            supporting = passwordHint(state.passwordProblem, state.repeat.isNotEmpty()),
        )
        Button(
            onClick = c::create,
            enabled = state.ready,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp),
        ) {
            if (state.busy) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
            Text(stringResource(R.string.admin_create_account), modifier = Modifier.padding(start = if (state.busy) 8.dp else 0.dp), style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun nameHint(problem: NameProblem?): String? = when (problem) {
    NameProblem.Empty -> stringResource(R.string.admin_name_empty)
    NameProblem.BadCharacters -> stringResource(R.string.admin_name_bad)
    null -> null
}

/** The domain the account is made in: a field that opens a menu of the server's domains. */
@Composable
private fun DomainPicker(state: NewAccountState, c: NewAccountController) {
    var open by remember { mutableStateOf(false) }
    val colors = MaterialTheme.colorScheme
    val current = state.domains.firstOrNull { it.id == state.domainId }
    Column {
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(Groups.OUTER_DP.dp))
                .background(colors.surfaceContainerHighest)
                .clickable { open = true }
                .heightIn(min = 56.dp)
                .padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.admin_domain_field), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
                Text(current?.name ?: stringResource(R.string.admin_domain_none), style = MaterialTheme.typography.bodyLarge)
            }
            Icon(Icons.Filled.ArrowDropDown, contentDescription = null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (domain in state.domains) {
                DropdownMenuItem(text = { Text(domain.name) }, onClick = { open = false; c.pickDomain(domain.id) })
            }
        }
    }
}
