package ovh.kweb.mail.ui.signin

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.WardMark

/**
 * The sign-in screen. [suggestedAddress] fills the field when signing in again to a known server;
 * [onBack] is given only then (the first sign-in has nowhere to go back to); [onCompleted] is called
 * when a sign-in finishes while this screen is still the one on display.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SignInScreen(
    suggestedAddress: String? = null,
    onBack: (() -> Unit)? = null,
    onCompleted: () -> Unit = {},
    vm: SignInViewModel = viewModel(),
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current

    LaunchedEffect(suggestedAddress) { vm.suggest(suggestedAddress) }
    LaunchedEffect(vm) {
        vm.urlsToOpen.collect { url -> if (!openInBrowser(context, url)) vm.browserUnavailable() }
    }
    LaunchedEffect(vm) { vm.completed.collect { onCompleted() } }

    Scaffold(
        topBar = {
            if (onBack != null) {
                TopAppBar(
                    title = {},
                    navigationIcon = {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                        }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
                )
            }
        },
    ) { padding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Header()
            Spacer(Modifier.height(32.dp))
            when (state.phase) {
                SignInPhase.Waiting -> Waiting(onCancel = vm::cancel)
                else -> Form(state, onAddress = vm::setAddress, onSignIn = vm::signIn)
            }
        }
    }
}

@Composable
private fun Header() {
    WardMark()
    Spacer(Modifier.height(16.dp))
    Text(stringResource(R.string.app_name), style = MaterialTheme.typography.headlineMedium)
    Spacer(Modifier.height(4.dp))
    Text(
        stringResource(R.string.signin_tagline),
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
    )
}

@Composable
private fun Form(state: SignInUiState, onAddress: (String) -> Unit, onSignIn: () -> Unit) {
    val starting = state.phase == SignInPhase.Starting
    OutlinedTextField(
        value = state.address,
        onValueChange = onAddress,
        modifier = Modifier.fillMaxWidth(),
        label = { Text(stringResource(R.string.signin_server_label)) },
        supportingText = { Text(state.error?.let { stringResource(it.messageRes(), *it.messageArgs()) } ?: stringResource(R.string.signin_server_hint)) },
        isError = state.error != null,
        enabled = !starting,
        singleLine = true,
        keyboardOptions = KeyboardOptions(
            capitalization = KeyboardCapitalization.None,
            autoCorrectEnabled = false,
            keyboardType = KeyboardType.Uri,
            imeAction = ImeAction.Go,
        ),
        keyboardActions = KeyboardActions(onGo = { onSignIn() }),
    )
    Spacer(Modifier.height(16.dp))
    Button(onClick = onSignIn, enabled = !starting, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
        if (starting) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
            Spacer(Modifier.padding(horizontal = 6.dp))
            Text(stringResource(R.string.signin_contacting))
        } else {
            Text(stringResource(R.string.signin_button))
        }
    }
}

@Composable
private fun Waiting(onCancel: () -> Unit) {
    CircularProgressIndicator()
    Spacer(Modifier.height(24.dp))
    Text(
        stringResource(R.string.signin_waiting),
        style = MaterialTheme.typography.bodyLarge,
        textAlign = TextAlign.Center,
    )
    Spacer(Modifier.height(24.dp))
    OutlinedButton(onClick = onCancel, modifier = Modifier.heightIn(min = 48.dp)) {
        Text(stringResource(R.string.cancel))
    }
}

@StringRes
private fun SignInError.messageRes(): Int = when (this) {
    SignInError.InvalidAddress -> R.string.signin_error_invalid_address
    SignInError.NoOAuth -> R.string.signin_error_no_oauth
    SignInError.Unreachable -> R.string.signin_error_unreachable
    SignInError.NoBrowserAnswer -> R.string.signin_error_no_answer
    SignInError.Refused -> R.string.signin_error_refused
    SignInError.NoBrowser -> R.string.signin_error_no_browser
    is SignInError.Other -> R.string.signin_error_other
}

private fun SignInError.messageArgs(): Array<Any> = if (this is SignInError.Other) arrayOf(detail) else emptyArray()
