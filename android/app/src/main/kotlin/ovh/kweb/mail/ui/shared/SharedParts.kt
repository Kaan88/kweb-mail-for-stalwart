package ovh.kweb.mail.ui.shared

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage

/** The top bar of every shared screen: a back arrow and a title. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SharedTopBar(title: String, onBack: () -> Unit, actions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {}) {
    TopAppBar(
        title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        navigationIcon = {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
            }
        },
        actions = actions,
        colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
    )
}

/**
 * What a controller tells its screen: its words go to the snackbar, a gone share to [onGone], a finished
 * action to [onDone]. The callbacks are the latest ones, whatever composition the collection started in.
 */
@Composable
internal fun CollectSharedEvents(
    events: Flow<SharedEvent>,
    snackbar: SnackbarHostState,
    onGone: () -> Unit,
    onDone: () -> Unit = {},
) {
    val resources = LocalResources.current
    val scope = rememberCoroutineScope()
    val gone by rememberUpdatedState(onGone)
    val done by rememberUpdatedState(onDone)
    LaunchedEffect(events) {
        events.collect { event ->
            when (event) {
                is SharedEvent.Say -> scope.launch {
                    snackbar.currentSnackbarData?.dismiss()
                    snackbar.showSnackbar(event.message.resolve(resources))
                }
                SharedEvent.Gone -> gone()
                SharedEvent.Done -> done()
            }
        }
    }
}

/** Content in the middle of the area, with room to pull down on (inside a lazy list). */
@Composable
internal fun LazyItemScope.SharedCentered(content: @Composable () -> Unit) {
    Column(
        Modifier.fillParentMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) { content() }
}

/** Content in the middle of a screen that does not scroll. */
@Composable
internal fun SharedCenteredBox(content: @Composable () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) { content() }
}

@Composable
internal fun Loading() {
    CircularProgressIndicator()
}

/** A plain note, and Retry when [onRetry] is given. */
@Composable
internal fun Note(text: String, onRetry: (() -> Unit)? = null) {
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
    if (onRetry != null) TextButton(onClick = onRetry) { Text(stringResource(R.string.retry)) }
}

@Composable
internal fun UiMessage.text(): String = resolve(LocalResources.current)

/** A small heading over a group, as the agenda's days have. */
@Composable
internal fun GroupHeading(text: String) {
    Text(
        text.uppercase(androidx.compose.ui.text.intl.Locale.current.platformLocale),
        modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 20.dp, bottom = 8.dp).semantics { heading() },
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

/**
 * One row of a group on the quiet surface: an optional picture, the title, an optional count. [indent] pushes a
 * nested folder to the right. [shape] is its place in the group (see `Groups`).
 */
@Composable
internal fun SharedRow(
    shape: Shape,
    title: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: Int? = null,
    count: String? = null,
    countDescription: String? = null,
    indentDp: Int = 0,
) {
    val colors = MaterialTheme.colorScheme
    Row(
        modifier
            .fillMaxWidth()
            // 12 dp from the sides and, with 1.5 dp above and below, the 3 dp gap between the rows of a group.
            .padding(horizontal = 12.dp, vertical = 1.5.dp)
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .clickable(role = Role.Button, onClick = onClick)
            .heightIn(min = 56.dp)
            .padding(start = (20 + indentDp).dp, end = 20.dp, top = 12.dp, bottom = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (icon != null) {
            Icon(painterResource(icon), contentDescription = null, tint = colors.onSurfaceVariant, modifier = Modifier.size(24.dp))
        }
        Text(title, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (count != null) {
            Text(
                count,
                modifier = Modifier.semantics { if (countDescription != null) contentDescription = countDescription },
                color = colors.onSurfaceVariant,
                style = MaterialTheme.typography.labelLarge,
            )
        }
    }
}

/** The room under the last row of a list, for the system bars. */
@Composable
internal fun BottomSpace() {
    Box(Modifier.size(1.dp, 24.dp))
}
