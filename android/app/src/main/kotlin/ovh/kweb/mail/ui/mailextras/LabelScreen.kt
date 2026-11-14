package ovh.kweb.mail.ui.mailextras

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.list.MailRow

/** The mail that carries one label, newest first; a tap opens the message in its thread. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LabelScreen(route: Route.Label, onBack: () -> Unit, onOpen: (threadId: String, messageId: String) -> Unit, vm: LabelViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val labels by vm.labels.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm, route.labelId) { vm.start(route.labelId) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(route.name, maxLines = 1) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            when (val results = state) {
                LabelResults.Loading -> Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.TopCenter) { CircularProgressIndicator() }
                LabelResults.Failed -> Note(stringResource(R.string.labels_read_failed)) {
                    TextButton(onClick = vm::reload) { Text(stringResource(R.string.retry)) }
                }
                is LabelResults.Found -> {
                    if (results.messages.isEmpty()) {
                        Note(stringResource(R.string.labels_list_empty)) {}
                    } else {
                        Column(Modifier.fillMaxSize()) {
                            if (results.fromCache) {
                                Surface(
                                    color = MaterialTheme.colorScheme.secondaryContainer,
                                    shape = RoundedCornerShape(14.dp),
                                    modifier = Modifier.padding(start = 14.dp, end = 14.dp, bottom = 8.dp).fillMaxWidth(),
                                ) {
                                    Text(
                                        stringResource(R.string.search_offline),
                                        modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
                                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                                        style = MaterialTheme.typography.bodyMedium,
                                    )
                                }
                            }
                            LazyColumn(
                                Modifier.fillMaxSize(),
                                contentPadding = PaddingValues(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 16.dp),
                                verticalArrangement = Arrangement.spacedBy(3.dp),
                            ) {
                                itemsIndexed(results.messages, key = { _, message -> message.id }) { index, message ->
                                    MailRow(
                                        messageRowContent(message, LabelRules.namesOf(labels.byMessage[message.id], labels.labels)),
                                        onClick = { onOpen(message.threadId, message.id) },
                                        shape = Groups.shape(index, results.messages.size),
                                    )
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
private fun Note(text: String, action: @Composable () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
        action()
    }
}
