package ovh.kweb.mail.ui.files

import android.content.ActivityNotFoundException
import android.content.Intent
import android.webkit.MimeTypeMap
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.ZoneId
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.sharing.LocalOpenShare
import ovh.kweb.mail.ui.sharing.ShareRules
import uniffi.kweb_core.ShareKind
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind
import uniffi.kweb_core.FileSpace

/** The words for a space: the user's own storage is "My files", the others are named by the owner's address. */
@Composable
internal fun spaceLabel(space: FileSpace): String = if (space.accountId == null) stringResource(R.string.files_my) else space.name

/**
 * The Files root: the folders and files of the server's file storage as grouped rows, with the bottom bar.
 * Back goes up a folder before it leaves the screen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FilesScreen(bottomBar: @Composable () -> Unit, appSnackbar: SnackbarHostState, vm: FilesViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val resources = LocalResources.current
    val locale = rememberLocale()
    val sizeUnits = SizeUnits(
        stringResource(R.string.files_unit_b),
        stringResource(R.string.files_unit_kb),
        stringResource(R.string.files_unit_mb),
        stringResource(R.string.files_unit_gb),
        stringResource(R.string.files_unit_tb),
    )
    val activity = LocalActivity.current
    val scope = rememberCoroutineScope()

    LifecycleResumeEffect(c) {
        c.onShown()
        onPauseOrDispose { }
    }
    // Leaving for another screen for good (not a rotation) closes the "Opening" window: the file must not pop up later.
    DisposableEffect(c) { onDispose { if (activity?.isChangingConfigurations != true) c.cancelOpening() } }

    val saver = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("*/*")) { target ->
        if (target == null) c.cancelSave() else c.save(target.toString())
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) c.upload(uris.map { it.toString() })
    }

    // Events are taken only while the screen is on show: a viewer cannot be started from the background, so a
    // file that arrives meanwhile waits in the channel until the user is back.
    val lifecycle = androidx.lifecycle.compose.LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(c, lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            c.events.collect { event ->
                when (event) {
                    is FilesEvent.Say -> scope.launch { appSnackbar.showSnackbar(event.message.resolve(resources)) }
                    is FilesEvent.View -> {
                        val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", event.file)
                        var type = event.mediaType
                        if (type == "application/octet-stream") {
                            val extension = event.file.name.substringAfterLast('.', "").lowercase()
                            type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension) ?: type
                        }
                        val intent = Intent(Intent.ACTION_VIEW)
                            .setDataAndType(uri, type)
                            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        try {
                            context.startActivity(intent)
                        } catch (_: ActivityNotFoundException) {
                            scope.launch { appSnackbar.showSnackbar(resources.getString(R.string.files_no_app)) }
                        }
                    }
                    is FilesEvent.TooLarge -> scope.launch {
                        appSnackbar.showSnackbar(
                            resources.getString(
                                R.string.files_too_large,
                                event.name,
                                FileRules.sizeText(event.size.toULong(), locale, sizeUnits),
                                FileRules.sizeText(event.limit, locale, sizeUnits),
                            ),
                        )
                    }
                    is FilesEvent.Unreadable -> scope.launch { appSnackbar.showSnackbar(resources.getString(R.string.files_unreadable, event.name)) }
                    is FilesEvent.UploadDone -> {
                        val done = event.tally.uploaded
                        val text = if (done == event.total) {
                            resources.getQuantityString(R.plurals.files_upload_done, done, done)
                        } else {
                            resources.getString(R.string.files_upload_summary, done, event.total)
                        }
                        scope.launch { appSnackbar.showSnackbar(text) }
                    }
                }
            }
        }
    }

    // Back closes the search, then goes up a folder; at the top it is the app's.
    BackHandler(enabled = state.query != null) { c.closeSearch() }
    BackHandler(enabled = state.query == null && state.canGoUp) { c.up() }

    val zone = remember { ZoneId.systemDefault() }
    val menuAdd = remember { mutableStateOf(false) }

    Scaffold(
        topBar = { FilesTopBar(state, c) },
        bottomBar = bottomBar,
        snackbarHost = { SnackbarHost(appSnackbar) },
        floatingActionButton = {
            if (state.mayAdd) {
                Box {
                    FloatingActionButton(
                        onClick = { menuAdd.value = true },
                        shape = CircleShape,
                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                    ) { Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.files_add)) }
                    DropdownMenu(expanded = menuAdd.value, onDismissRequest = { menuAdd.value = false }) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.files_upload)) },
                            onClick = {
                                menuAdd.value = false
                                picker.launch(arrayOf("*/*"))
                            },
                        )
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.files_new_folder)) },
                            onClick = {
                                menuAdd.value = false
                                c.askNewFolder()
                            },
                        )
                    }
                }
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.upload?.let { UploadBar(it, onStop = c::stopUpload) }
            val space = state.space
            if (state.query == null && space != null && state.trail.isNotEmpty() && state.trailConfirmed) {
                Text(
                    FileRules.pathLine(spaceLabel(space), state.trail),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 4.dp),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.StartEllipsis,
                )
            }
            Body(
                state = state,
                c = c,
                zone = zone,
                locale = locale,
                sizeUnits = sizeUnits,
                onAction = { entry, action ->
                    when (action) {
                        RowAction.Open -> c.open(entry)
                        RowAction.Save -> {
                            c.pendingSave = entry
                            saver.launch(entry.name)
                        }
                        RowAction.Rename -> c.askRename(entry)
                        RowAction.Move -> c.askMove(entry)
                        RowAction.Delete -> c.askDelete(entry)
                    }
                },
            )
        }
    }
    FilesDialogHost(state, c)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FilesTopBar(state: FilesState, c: FilesController) {
    val colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background)
    val query = state.query
    if (query != null) {
        TopAppBar(
            title = { SearchField(query, c::setQuery) },
            navigationIcon = {
                IconButton(onClick = c::closeSearch) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.files_search_close))
                }
            },
            actions = {
                if (query.isNotEmpty()) {
                    IconButton(onClick = { c.setQuery("") }) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.files_search_clear))
                    }
                }
            },
            colors = colors,
        )
        return
    }
    val space = state.space
    val title = when {
        state.trail.isNotEmpty() -> state.trail.last().name
        space != null && (state.spaces?.size ?: 0) > 1 -> spaceLabel(space)
        else -> stringResource(R.string.files_title)
    }
    TopAppBar(
        title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        navigationIcon = {
            if (state.canGoUp) {
                IconButton(onClick = c::up) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.files_up))
                }
            }
        },
        actions = {
            if (space != null) {
                IconButton(onClick = c::openSearch) {
                    Icon(Icons.Filled.Search, contentDescription = stringResource(R.string.files_search))
                }
            }
        },
        colors = colors,
    )
}

@Composable
private fun SearchField(text: String, onChange: (String) -> Unit) {
    val colors = MaterialTheme.colorScheme
    val focus = remember { FocusRequester() }
    val label = stringResource(R.string.files_search)
    LaunchedEffect(Unit) { focus.requestFocus() }
    Box(contentAlignment = Alignment.CenterStart) {
        if (text.isEmpty()) Text(label, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
        BasicTextField(
            value = text,
            onValueChange = onChange,
            singleLine = true,
            textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.onSurface),
            cursorBrush = SolidColor(colors.primary),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { onChange(text) }),
            modifier = Modifier.fillMaxWidth().focusRequester(focus).semantics { contentDescription = label },
        )
    }
}

@Composable
private fun UploadBar(upload: UploadState, onStop: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 4.dp)
            .clip(MaterialTheme.shapes.large)
            .background(colors.secondaryContainer)
            .padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    stringResource(R.string.files_uploading, upload.position, upload.total),
                    color = colors.onSecondaryContainer,
                    style = MaterialTheme.typography.labelLarge,
                )
                val second = if (upload.stopping) stringResource(R.string.files_stopping) else upload.name
                if (second.isNotEmpty()) {
                    Text(second, color = colors.onSecondaryContainer, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            TextButton(onClick = onStop, enabled = !upload.stopping) { Text(stringResource(R.string.files_stop), color = colors.onSecondaryContainer) }
        }
        LinearProgressIndicator(Modifier.fillMaxWidth().padding(end = 8.dp, top = 4.dp))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Body(
    state: FilesState,
    c: FilesController,
    zone: ZoneId,
    locale: java.util.Locale,
    sizeUnits: SizeUnits,
    onAction: (FileEntry, RowAction) -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    val resources = LocalResources.current
    val query = state.query
    val spaces = state.spaces
    when {
        // Searching: the answer to the typed text.
        query != null -> {
            val results = state.results
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
                when {
                    state.searchError != null -> item(key = "error") {
                        Centered {
                            Text(state.searchError.resolve(resources), color = colors.onSurfaceVariant, textAlign = TextAlign.Center)
                            TextButton(onClick = { c.setQuery(query) }) { Text(stringResource(R.string.retry)) }
                        }
                    }
                    state.searching && results == null -> item(key = "loading") { Centered { CircularProgressIndicator() } }
                    results == null -> item(key = "hint") { Centered { Hint(stringResource(R.string.files_search_hint)) } }
                    results.isEmpty() -> item(key = "none") { Centered { Hint(stringResource(R.string.files_search_none)) } }
                    else -> itemsIndexed(results, key = { _, e -> "r/${e.id}" }) { index, entry ->
                        val where = entry.parentId?.let { state.folderNames[it] }?.let { resources.getString(R.string.files_in_folder, it) }
                        EntryRow(entry, index, results.size, subtitle = listOfNotNull(FileRules.subtitle(entry, zone, locale, sizeUnits).ifEmpty { null }, where).joinToString(" · "), onAction)
                    }
                }
            }
        }
        spaces == null -> {
            if (state.error != null) FailedBlock(state.error.resolve(resources), onRetry = c::refresh) else Box(Modifier.fillMaxSize(), Alignment.Center) { CircularProgressIndicator() }
        }
        spaces.isEmpty() -> Box(Modifier.fillMaxSize().padding(32.dp), Alignment.Center) { Hint(stringResource(R.string.files_no_storage)) }
        state.space == null -> LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = 8.dp, bottom = 96.dp)) {
            itemsIndexed(spaces, key = { _, s -> "s/${s.accountId}" }) { index, space ->
                GroupRow(
                    index,
                    spaces.size,
                    onClick = { c.selectSpace(space) },
                    icon = if (space.accountId == null) FileIcon.Folder else FileIcon.Other,
                    shared = space.accountId != null,
                    title = spaceLabel(space),
                    subtitle = if (space.accountId == null) "" else stringResource(R.string.files_shared_with_you),
                )
            }
        }
        else -> {
            val listing = state.listing
            Column(Modifier.fillMaxSize()) {
                if (state.error != null && listing != null) {
                    NoticeBar(state.error.resolve(resources), error = true, action = stringResource(R.string.retry) to c::refresh)
                }
                PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = c::refresh, modifier = Modifier.weight(1f).fillMaxWidth()) {
                    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = 4.dp, bottom = 96.dp)) {
                        when {
                            listing == null && state.error != null -> item(key = "failed") {
                                Centered {
                                    Text(state.error.resolve(resources), color = colors.onSurfaceVariant, textAlign = TextAlign.Center)
                                    TextButton(onClick = c::refresh) { Text(stringResource(R.string.retry)) }
                                }
                            }
                            listing == null -> item(key = "loading") { Centered { CircularProgressIndicator() } }
                            listing.entries.isEmpty() -> item(key = "empty") { Centered { Hint(stringResource(R.string.files_empty)) } }
                            else -> itemsIndexed(listing.entries, key = { _, e -> e.id }) { index, entry ->
                                EntryRow(entry, index, listing.entries.size, subtitle = FileRules.subtitle(entry, zone, locale, sizeUnits), onAction)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
}

@Composable
private fun FailedBlock(text: String, onRetry: () -> Unit) {
    Column(Modifier.fillMaxSize().padding(32.dp), Arrangement.Center, Alignment.CenterHorizontally) {
        Hint(text)
        TextButton(onClick = onRetry) { Text(stringResource(R.string.retry)) }
    }
}

/** Content in the middle of the list area, with room to pull down on. */
@Composable
private fun LazyItemScope.Centered(content: @Composable () -> Unit) {
    Column(
        Modifier.fillParentMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) { content() }
}

/** The tonal round badge with the picture of the kind of entry. */
@Composable
internal fun FileIconBadge(icon: FileIcon, size: Int = 44, shared: Boolean = false) {
    val colors = MaterialTheme.colorScheme
    val res = when {
        shared -> R.drawable.ic_group
        icon == FileIcon.Folder -> R.drawable.ic_folder
        icon == FileIcon.Picture -> R.drawable.ic_file_image
        icon == FileIcon.Text -> R.drawable.ic_file_text
        icon == FileIcon.Pdf -> R.drawable.ic_file_pdf
        else -> R.drawable.ic_file_other
    }
    val folder = icon == FileIcon.Folder || shared
    Box(
        Modifier.size(size.dp).clip(CircleShape).background(if (folder) colors.primaryContainer else colors.secondaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            painterResource(res),
            contentDescription = null,
            tint = if (folder) colors.onPrimaryContainer else colors.onSecondaryContainer,
            modifier = Modifier.size((size / 2 + 2).dp),
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun GroupRow(
    index: Int,
    count: Int,
    onClick: () -> Unit,
    icon: FileIcon,
    title: String,
    subtitle: String,
    shared: Boolean = false,
    onLongClick: (() -> Unit)? = null,
    endPadding: androidx.compose.ui.unit.Dp = 16.dp,
    trailing: @Composable () -> Unit = {},
) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp)
            .padding(top = if (index == 0) 0.dp else 3.dp)
            .clip(Groups.shape(index, count))
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .combinedClickable(role = Role.Button, onClick = onClick, onLongClick = onLongClick)
            .padding(start = 16.dp, end = endPadding, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        FileIconBadge(icon, shared = shared)
        Column(Modifier.weight(1f).padding(vertical = 2.dp)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (subtitle.isNotEmpty()) {
                Text(
                    subtitle,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        trailing()
    }
}

/** One file or folder, with its menu (the three dots, or a long press) for what its rights allow. */
@Composable
private fun EntryRow(entry: FileEntry, index: Int, count: Int, subtitle: String, onAction: (FileEntry, RowAction) -> Unit) {
    val actions = remember(entry) { FileRules.rowActions(entry) }
    // "Share" for a folder of the user's own space; the entries of a shared space are not the user's to share.
    val openShare = LocalOpenShare.current
    val shareable = ShareRules.fileFolderShareable(entry)
    var menu by remember { mutableStateOf(false) }
    val labels = mapOf(
        RowAction.Open to R.string.files_open,
        RowAction.Save to R.string.files_save,
        RowAction.Rename to R.string.files_rename,
        RowAction.Move to R.string.files_move,
        RowAction.Delete to R.string.files_delete,
    )
    Box {
        GroupRow(
            index = index,
            count = count,
            onClick = { onAction(entry, RowAction.Open) },
            icon = FileRules.iconFor(entry.kind, entry.mediaType, entry.name),
            title = entry.name,
            subtitle = subtitle,
            onLongClick = if (actions.isEmpty() && !shareable) null else ({ menu = true }),
            endPadding = if (actions.isEmpty() && !shareable) 16.dp else 4.dp,
            trailing = {
                if (actions.isNotEmpty() || shareable) {
                    IconButton(onClick = { menu = true }) {
                        Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.files_more, entry.name))
                    }
                }
            },
        )
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            for (action in actions) {
                DropdownMenuItem(
                    text = { Text(stringResource(labels.getValue(action))) },
                    onClick = {
                        menu = false
                        onAction(entry, action)
                    },
                )
            }
            if (shareable) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.share_menu)) },
                    onClick = {
                        menu = false
                        openShare(ShareKind.FILE_FOLDER, entry.id, entry.name)
                    },
                )
            }
        }
    }
}
