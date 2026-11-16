package ovh.kweb.mail.ui.home

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.accounts.AccountAvatarButton
import ovh.kweb.mail.ui.attention.AttentionNav
import ovh.kweb.mail.ui.attention.AttentionRules
import ovh.kweb.mail.ui.attention.AttentionSheet
import ovh.kweb.mail.ui.attention.AttentionViewModel
import ovh.kweb.mail.ui.attention.AttentionWords
import ovh.kweb.mail.ui.attention.Group
import ovh.kweb.mail.ui.attention.ResourceTexts
import ovh.kweb.mail.ui.attention.SparkleButton
import ovh.kweb.mail.ui.attention.SparkleMark
import ovh.kweb.mail.ui.attention.WordClock
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.TileSwitches

/** What the Home tab can open; each tile opens its place. */
class HomeActions(
    val openMail: () -> Unit,
    val openFlagged: () -> Unit,
    val openCalendar: () -> Unit,
    val openFiles: () -> Unit,
    val openSearch: () -> Unit,
    val openDeliveryCheck: () -> Unit,
    val openAway: () -> Unit,
    /** The Tasks view of the Calendar tab (the Due today tile); the Calendar when not given. */
    val openTasks: () -> Unit = openCalendar,
)

/**
 * The Home tab: a greeting, the status chips (only while true), and the tiles. A tile gets the sparkle with a count when
 * a rule fired for it; a tap on the sparkle opens the sheet "Worth a look" at that tile's group.
 */
@Composable
fun HomeScreen(
    actions: HomeActions,
    nav: AttentionNav,
    bottomBar: @Composable () -> Unit,
    appSnackbar: SnackbarHostState,
    vm: HomeViewModel = viewModel(),
    attention: AttentionViewModel = viewModel(),
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val found by attention.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val texts = remember(resources) { ResourceTexts(resources) }
    var sheet by remember { mutableStateOf<SheetRequest?>(null) }

    LifecycleResumeEffect(vm) {
        vm.load()
        attention.load()
        onPauseOrDispose { }
    }
    LaunchedEffect(attention) {
        attention.notices.collect { notice ->
            snackbar.currentSnackbarData?.dismiss()
            val result = snackbar.showSnackbar(
                notice.text,
                actionLabel = notice.undo?.let { resources.getString(R.string.att_undo) },
                duration = if (notice.undo != null) SnackbarDuration.Long else SnackbarDuration.Short,
            )
            if (result == SnackbarResult.ActionPerformed) notice.undo?.invoke()
        }
    }

    val tiles = found.settings?.tiles ?: ALL_TILES
    val now = state.at.takeIf { it > 0 } ?: (System.currentTimeMillis() / 1000)
    val clock = remember(now) { WordClock(ZoneId.systemDefault(), Locale.getDefault(), now) }
    val marked = remember(found.findings) { AttentionRules.markedMessages(found.findings) }
    fun sparks(tile: HomeTile) = AttentionRules.count(found.findings, tile)
    fun openSheet(tile: HomeTile) {
        sheet = SheetRequest(AttentionRules.groupOfTile(tile))
    }

    Scaffold(
        topBar = { HomeTopBar(now, onSearch = actions.openSearch) },
        bottomBar = bottomBar,
        snackbarHost = {
            Column {
                SnackbarHost(snackbar)
                SnackbarHost(appSnackbar)
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(
            Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val chips = (tiles.delivery && state.problem != null) || (tiles.away && state.away)
            if (chips) {
                Row(Modifier.padding(horizontal = 2.dp).padding(bottom = 2.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (tiles.delivery && state.problem != null) {
                        val since = state.problemSince
                        val text = when {
                            state.problem == HomeRules.Problem.SIGN_IN -> stringResource(R.string.home_status_sign_in)
                            since != null -> stringResource(R.string.home_status_offline_since, AttentionWords.time(since, clock))
                            else -> stringResource(R.string.home_status_offline)
                        }
                        StatusChip(text, rememberVectorPainter(Icons.Filled.Warning), MaterialTheme.colorScheme.errorContainer, MaterialTheme.colorScheme.onErrorContainer, actions.openDeliveryCheck)
                    }
                    if (tiles.away && state.away) {
                        val until = state.awayUntil
                        val text = if (until != null) {
                            stringResource(R.string.home_status_away_until, AttentionWords.time(until, clock))
                        } else {
                            stringResource(R.string.home_status_away)
                        }
                        StatusChip(text, rememberVectorPainter(Icons.AutoMirrored.Filled.Send), MaterialTheme.colorScheme.secondaryContainer, MaterialTheme.colorScheme.onSecondaryContainer, actions.openAway)
                    }
                }
            }

            if (tiles.unread) {
                UnreadTile(state, marked, sparks(HomeTile.UNREAD), actions.openMail) { openSheet(HomeTile.UNREAD) }
            }
            val showEvent = tiles.nextEvent
            val showFlagged = tiles.flagged
            val showTasks = tiles.dueToday
            if (showEvent || showFlagged || showTasks) {
                Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (showEvent || showFlagged) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (showEvent) EventTile(state, clock, texts, sparks(HomeTile.NEXT_EVENT), actions.openCalendar) { openSheet(HomeTile.NEXT_EVENT) }
                            if (showFlagged) FlaggedTile(state, actions.openFlagged)
                        }
                    }
                    if (showTasks) {
                        TasksTile(state, sparks(HomeTile.DUE_TODAY), Modifier.weight(1f).fillMaxHeight(), actions.openTasks) { openSheet(HomeTile.DUE_TODAY) }
                    }
                }
            }
            val files = state.files
            if (tiles.recentFiles && files != null) FilesTile(files, clock, actions.openFiles)
        }
    }

    sheet?.let { request ->
        AttentionSheet(attention, nav, request.group, onClose = { sheet = null })
    }
}

private val ALL_TILES = TileSwitches(true, true, true, true, true, true, true)

private data class SheetRequest(val group: Group)

@Composable
private fun HomeTopBar(now: Long, onSearch: () -> Unit) {
    val hour = Instant.ofEpochSecond(now).atZone(ZoneId.systemDefault()).hour
    val greeting = when (HomeRules.greeting(hour)) {
        HomeRules.Greeting.MORNING -> R.string.home_greeting_morning
        HomeRules.Greeting.AFTERNOON -> R.string.home_greeting_afternoon
        HomeRules.Greeting.EVENING -> R.string.home_greeting_evening
    }
    val date = remember(now) { DateTimeFormatter.ofPattern("EEEE d MMMM", Locale.getDefault()).format(Instant.ofEpochSecond(now).atZone(ZoneId.systemDefault())) }
    Row(
        Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background).statusBarsPadding().padding(start = 24.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).semantics { heading() }) {
            Text(stringResource(greeting), style = MaterialTheme.typography.headlineSmall)
            Text(date, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        IconButton(onClick = onSearch) { Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.home_search)) }
        AccountAvatarButton(Modifier.padding(start = 4.dp, end = 10.dp))
    }
}

@Composable
private fun StatusChip(text: String, icon: Painter, container: Color, content: Color, onClick: () -> Unit) {
    Row(
        Modifier.height(32.dp).clip(RoundedCornerShape(8.dp)).background(container).clickable(onClick = onClick).padding(start = 8.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(icon, contentDescription = null, tint = content, modifier = Modifier.size(17.dp))
        Text(text, color = content, fontSize = 12.5.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** One tile: a header (icon, title, the sparkle when something was found) over free content. */
@Composable
private fun Tile(
    title: String,
    icon: Painter,
    container: Color,
    content: Color,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    sparks: Int = 0,
    onSpark: () -> Unit = {},
    body: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(container).clickable(onClick = onClick).padding(start = 14.dp, end = 14.dp, top = 8.dp, bottom = 13.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Row(Modifier.heightIn(min = 32.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Icon(icon, contentDescription = null, tint = content.copy(alpha = 0.85f), modifier = Modifier.size(18.dp))
            Text(title, modifier = Modifier.weight(1f), style = MaterialTheme.typography.labelLarge, color = content.copy(alpha = 0.85f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            SparkleButton(sparks, onSpark, size = 32.dp, modifier = Modifier.padding(end = 4.dp))
        }
        body()
    }
}

@Composable
private fun UnreadTile(state: HomeState, marked: Set<String>, sparks: Int, onClick: () -> Unit, onSpark: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Tile(
        stringResource(R.string.home_tile_unread), painterResource(R.drawable.ic_mail), colors.primaryContainer, colors.onPrimaryContainer, onClick,
        sparks = sparks, onSpark = onSpark,
    ) {
        Row(verticalAlignment = Alignment.Top) {
            Text(state.unread.toString(), fontSize = 36.sp, lineHeight = 36.sp, color = colors.onPrimaryContainer)
            Column(Modifier.weight(1f).padding(start = 12.dp, top = 2.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                if (state.newest.isEmpty()) {
                    Text(stringResource(R.string.home_unread_none), style = MaterialTheme.typography.bodySmall, color = colors.onPrimaryContainer.copy(alpha = 0.8f))
                }
                for (m in state.newest) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(
                            androidx.compose.ui.text.buildAnnotatedString {
                                pushStyle(androidx.compose.ui.text.SpanStyle(fontWeight = FontWeight.Bold))
                                append(HomeRules.sender(m))
                                pop()
                                append(" ")
                                append(m.subject)
                            },
                            modifier = Modifier.weight(1f),
                            style = MaterialTheme.typography.bodySmall,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        if (m.id in marked) SparkleMark(tint = colors.onPrimaryContainer)
                    }
                }
            }
        }
    }
}

@Composable
private fun EventTile(state: HomeState, clock: WordClock, texts: ResourceTexts, sparks: Int, onClick: () -> Unit, onSpark: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Tile(
        stringResource(R.string.home_tile_next_event), painterResource(R.drawable.ic_calendar), colors.surfaceContainerLow, colors.onSurface, onClick,
        sparks = sparks, onSpark = onSpark,
    ) {
        val next = state.next
        if (next == null) {
            Text(stringResource(R.string.home_event_none), style = MaterialTheme.typography.titleMedium)
        } else {
            val until = next.startsAt - clock.nowSeconds
            Text(
                if (until <= 0) stringResource(R.string.home_event_now) else stringResource(R.string.home_event_in, AttentionWords.span(until, texts)),
                style = MaterialTheme.typography.titleLarge,
            )
            Text(
                "${next.title}, ${AttentionWords.time(next.startsAt, clock)}",
                style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
            state.then?.let {
                Text(stringResource(R.string.home_event_then, it.title), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun TasksTile(state: HomeState, sparks: Int, modifier: Modifier, onClick: () -> Unit, onSpark: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Tile(
        stringResource(R.string.home_tile_due_today), rememberVectorPainter(Icons.Filled.CheckCircle), colors.surfaceContainerLow, colors.onSurface, onClick,
        modifier = modifier, sparks = sparks, onSpark = onSpark,
    ) {
        val due = state.tasksToday
        Text(
            if (due.isEmpty()) stringResource(R.string.home_tasks_none) else pluralStringResource(R.plurals.home_tasks, due.size, due.size),
            style = MaterialTheme.typography.titleLarge,
        )
        Column(Modifier.padding(top = 6.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            for (task in due.take(4)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(9.dp)) {
                    Box(Modifier.size(16.dp).border(2.dp, colors.outline, CircleShape))
                    Text(task.title, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        androidx.compose.foundation.layout.Spacer(Modifier.weight(1f))
        val more = when {
            state.overdue > 0 -> stringResource(R.string.home_tasks_more, state.overdue)
            state.tomorrow > 0 -> stringResource(R.string.home_tasks_tomorrow, state.tomorrow)
            else -> null
        }
        if (more != null) Text(more, style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, modifier = Modifier.padding(top = 4.dp))
    }
}

@Composable
private fun FlaggedTile(state: HomeState, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Tile(stringResource(R.string.home_tile_flagged), rememberVectorPainter(Icons.Filled.Star), colors.surfaceContainerLow, colors.onSurface, onClick) {
        if (state.flagged == 0) {
            Text(stringResource(R.string.home_flagged_none), style = MaterialTheme.typography.titleMedium)
        } else {
            Text(pluralStringResource(R.plurals.home_flagged_count, state.flagged, state.flagged), style = MaterialTheme.typography.titleLarge)
            state.oldestFlaggedDays?.let {
                val age = if (it == 0) stringResource(R.string.home_today) else pluralStringResource(R.plurals.home_days, it, it)
                Text(stringResource(R.string.home_flagged_oldest, age), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, maxLines = 1)
            }
        }
    }
}

@Composable
private fun FilesTile(files: List<FileEntry>, clock: WordClock, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Tile(stringResource(R.string.home_tile_files), painterResource(R.drawable.ic_files), colors.surfaceContainerHigh, colors.onSurface, onClick) {
        for (file in files) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    extensionOf(file.name),
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(colors.secondaryContainer).padding(horizontal = 7.dp, vertical = 1.dp),
                    color = colors.onSecondaryContainer, fontSize = 10.5.sp, fontWeight = FontWeight.Bold, maxLines = 1,
                )
                Text(file.name, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                file.modified?.let { Text(AttentionWords.time(it, clock), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, maxLines = 1) }
            }
        }
    }
}

/** "PDF" for "Contract-v3.pdf"; "FILE" when there is no extension. */
internal fun extensionOf(name: String): String = name.substringAfterLast('.', "").take(4).uppercase().ifEmpty { "FILE" }
