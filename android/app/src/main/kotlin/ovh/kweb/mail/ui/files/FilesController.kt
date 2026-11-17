package ovh.kweb.mail.ui.files

import java.io.File
import java.io.IOException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.transfer.DownloadProgress
import ovh.kweb.mail.ui.transfer.DownloadRules
import ovh.kweb.mail.ui.transfer.DownloadStop
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind
import uniffi.kweb_core.FileListing
import uniffi.kweb_core.FileSpace
import uniffi.kweb_core.PublicLink
import uniffi.kweb_core.PublicLinkLimits

/** A window over a change the user is making. */
sealed interface FilesDialog {
    data object NewFolder : FilesDialog
    data class Rename(val entry: FileEntry) : FilesDialog
    data class Delete(val entry: FileEntry) : FilesDialog

    /** Choosing where [entry] goes: [trail] is the way down to the folder shown, [listing] its content (null while it loads). */
    data class Move(
        val entry: FileEntry,
        val trail: List<FileEntry>,
        val listing: FileListing? = null,
        val error: UiMessage? = null,
    ) : FilesDialog

    /**
     * The public links of [entry]. [links] is null while they load; [revoking] is the link the user is asked about;
     * [copiedId] the link last put on the clipboard; [error] says why something failed.
     */
    data class PublicLinks(
        val entry: FileEntry,
        val validity: LinkValidity,
        val links: List<PublicLink>? = null,
        val revoking: PublicLink? = null,
        val working: Boolean = false,
        val copiedId: String? = null,
        val error: UiMessage? = null,
    ) : FilesDialog
}

/** "name" is already there; [existing] says what is there. */
data class UploadConflict(val name: String, val existing: FileEntry)

/** An upload under way: file [position] of [total], and what it waits for. */
data class UploadState(
    val position: Int,
    val total: Int,
    val name: String,
    val stopping: Boolean = false,
    val conflict: UploadConflict? = null,
)

data class FilesState(
    /** The places that hold files; null until the server has answered. */
    val spaces: List<FileSpace>? = null,
    /** The place shown; null shows the choice of places (several) or nothing (none). */
    val space: FileSpace? = null,
    /** The folders from the top of the space down to the one shown. */
    val trail: List<FileEntry> = emptyList(),
    /** False while [trail] is only a guess (a folder opened from a search, before its listing has said where it is). */
    val trailConfirmed: Boolean = true,
    val listing: FileListing? = null,
    /** Nothing to show yet for this place: spaces or the first listing are on their way. */
    val loading: Boolean = false,
    val refreshing: Boolean = false,
    val error: UiMessage? = null,
    /** The text in the search field; null when not searching. */
    val query: String? = null,
    val results: List<FileEntry>? = null,
    val searching: Boolean = false,
    val searchError: UiMessage? = null,
    /** The names of folders seen so far, to say where a search result is. */
    val folderNames: Map<String, String> = emptyMap(),
    val dialog: FilesDialog? = null,
    val dialogError: UiMessage? = null,
    /** A change (rename, move, delete, new folder) is on its way. */
    val busy: Boolean = false,
    /** The name of the file being downloaded for opening or saving. */
    val opening: String? = null,
    /** The download is for "Save to device", not for opening in another app. */
    val openingToSave: Boolean = false,
    /** How far that download is; its total is null when neither the server nor the list says. */
    val openingProgress: DownloadProgress? = null,
    val upload: UploadState? = null,
    /** The server's limits for public links; null while unknown or when the server has none (the entry is then not offered). */
    val publicLinkLimits: PublicLinkLimits? = null,
) {
    /** The listing is for the top level or a folder, not the choice of spaces. */
    val atSpaceChoice: Boolean get() = space == null && (spaces?.size ?: 0) > 1

    /** The back arrow has somewhere to go: up a folder, or from the top of a space to the choice of spaces. */
    val canGoUp: Boolean get() = space != null && (trail.isNotEmpty() || (spaces?.size ?: 0) > 1)

    /** A file or folder may be created in the place shown. */
    val mayAdd: Boolean get() = space != null && query == null && (listing?.mayAdd ?: false)
}

sealed interface FilesEvent {
    /** Something to tell the user, as a snackbar. */
    data class Say(val message: UiMessage) : FilesEvent

    /** A downloaded file is ready to be opened by another app. */
    data class View(val file: File, val mediaType: String) : FilesEvent

    /** A file is more than the server takes ([limit] bytes) and was not sent. */
    data class TooLarge(val name: String, val size: Long, val limit: ULong) : FilesEvent

    /** A picked document could not be read. */
    data class Unreadable(val name: String) : FilesEvent

    /** A public link was made: the screen puts [url] on the clipboard. The link is never logged. */
    data class CopyLink(val url: String) : FilesEvent

    /** An upload ended. */
    data class UploadDone(val total: Int, val tally: UploadTally) : FilesEvent
}

/**
 * The logic of the Files area, free of Android so that it can be tested with fakes. All calls to [backend] and
 * [platform] run on [io]; their answers are applied on [scope]'s dispatcher. Only the newest request for a
 * listing, a search or the move dialog counts: an older answer that comes late is dropped.
 */
class FilesController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: FilesBackend,
    private val platform: FilesPlatform,
    private val searchDelayMillis: Long = 300,
) {
    private val flow = MutableStateFlow(FilesState())
    private val channel = Channel<FilesEvent>(Channel.UNLIMITED)

    val state: StateFlow<FilesState> get() = flow
    val events: Flow<FilesEvent> get() = channel.receiveAsFlow()

    private var spacesGen = 0
    private var listGen = 0
    private var searchGen = 0
    private var moveGen = 0
    private var linksGen = 0
    private var limitsGen = 0
    private var limitsAsked = false
    private var searchJob: Job? = null
    private var openJob: Job? = null
    private var openGen = 0
    /** Ends the core's download of the file being opened. */
    private var openStop: DownloadStop? = null
    /** The search text to go back to from a folder opened out of a search whose place is not yet confirmed. */
    private var returnQuery: String? = null
    private var uploadJob: Job? = null
    private var conflictAnswer: CompletableDeferred<ConflictChoice>? = null

    @Volatile
    private var stopRequested = false

    /** The file the save picker is open for. */
    var pendingSave: FileEntry? = null

    init {
        // Copies a cancelled or killed upload left behind.
        scope.launch { call { platform.sweepLeftovers() } }
    }

    private suspend fun <T> call(block: () -> T): Result<T> = try {
        Result.success(withContext(io) { block() })
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e)
    }

    private fun say(message: UiMessage) {
        channel.trySend(FilesEvent.Say(message))
    }

    private fun say(res: Int) = say(UiMessage.Res(res))

    /** The screen came up (also from the background): ask for the places the first time, else read the folder again quietly. */
    fun onShown() {
        val s = flow.value
        when {
            s.spaces == null -> loadSpaces(reloadKept = true)
            // At the choice of places, or with none: they may have changed (a folder was shared, or withdrawn).
            s.space == null -> loadSpaces(reloadKept = true)
            else -> {
                load(clear = false, refreshing = false)
                if (s.trail.isEmpty()) loadSpaces(reloadKept = false)
            }
        }
        if (s.query != null && !s.query.isBlank()) setQuery(s.query)
    }

    /** Pull to refresh, and Retry. */
    fun refresh() {
        val s = flow.value
        if (s.space == null) {
            loadSpaces(reloadKept = true)
        } else {
            load(clear = s.listing == null, refreshing = s.listing != null)
            // At the top the list of places is read again too: a share may have come or gone.
            if (s.trail.isEmpty()) loadSpaces(reloadKept = false)
        }
    }

    /** Signed out: forget everything and stop what runs. */
    fun reset() {
        spacesGen++
        listGen++
        searchGen++
        moveGen++
        linksGen++
        limitsGen++
        limitsAsked = false
        searchJob?.cancel()
        openJob?.cancel()
        openGen++
        uploadJob?.cancel()
        conflictAnswer?.cancel()
        returnQuery = null
        flow.value = FilesState()
        scope.launch { call { platform.sweepLeftovers() } }
    }

    // ---- places and folders

    /** Asks for the places. [reloadKept] says whether the folder shown is read again when its place is still there. */
    private fun loadSpaces(reloadKept: Boolean) {
        val gen = ++spacesGen
        // Only the first read shows progress or an error; a later one is quiet and keeps what is on screen.
        flow.update { if (it.spaces == null) it.copy(loading = true, error = null) else it }
        scope.launch {
            val result = call { backend.spaces() }
            if (gen != spacesGen) return@launch
            result.onSuccess { list ->
                val kept = flow.value.space?.let { current -> list.firstOrNull { it.accountId == current.accountId } }
                val space = kept ?: list.singleOrNull()
                flow.update {
                    it.copy(
                        spaces = list,
                        space = space,
                        trail = if (kept != null) it.trail else emptyList(),
                        trailConfirmed = if (kept != null) it.trailConfirmed else true,
                        listing = if (kept != null) it.listing else null,
                        loading = if (kept != null) it.loading else space != null,
                        error = null,
                    )
                }
                if (space != null && (kept == null || reloadKept)) load(clear = kept == null, refreshing = false)
                if (list.any { it.accountId == null }) loadLimits()
            }.onFailure { e ->
                if (flow.value.spaces == null) flow.update { it.copy(loading = false, refreshing = false, error = FilesWords.message(e)) }
            }
        }
    }

    /** Asks once whether the server has public links. A failure counts as "not yet known" and is asked again later; null is final. */
    private fun loadLimits() {
        if (limitsAsked) return
        limitsAsked = true
        val gen = limitsGen
        scope.launch {
            val result = call { backend.publicLinkLimits() }
            if (gen != limitsGen) return@launch
            result.onSuccess { limits -> flow.update { it.copy(publicLinkLimits = limits) } }.onFailure { limitsAsked = false }
        }
    }

    fun selectSpace(space: FileSpace) {
        closeSearch()
        flow.update { it.copy(space = space, trail = emptyList(), trailConfirmed = true, listing = null) }
        load(clear = true, refreshing = false)
    }

    fun openFolder(folder: FileEntry) {
        if (folder.kind != FileKind.FOLDER) return
        val fromSearch = flow.value.query != null
        if (fromSearch) returnQuery = flow.value.query?.takeIf { it.isNotBlank() }
        if (fromSearch) closeSearch()
        // A folder from a search may be anywhere in the space: its place is only known once its listing arrives.
        flow.update {
            if (fromSearch) it.copy(trail = listOf(folder), trailConfirmed = false, listing = null)
            else it.copy(trail = it.trail + folder, listing = null)
        }
        load(clear = true, refreshing = false)
    }

    /** The back arrow and system back: up a folder, or from the top of a space to the choice of places. */
    fun up() {
        val s = flow.value
        if (s.trail.isNotEmpty() && !s.trailConfirmed) {
            // We do not know where this folder is: back goes to the search it was found in.
            val q = returnQuery
            returnQuery = null
            listGen++
            flow.update { it.copy(trail = emptyList(), trailConfirmed = true, listing = null, error = null) }
            load(clear = true, refreshing = false)
            if (q != null) {
                openSearch()
                setQuery(q)
            }
        } else if (s.trail.isNotEmpty()) {
            flow.update { it.copy(trail = it.trail.dropLast(1), listing = null) }
            load(clear = true, refreshing = false)
        } else if (s.space != null && (s.spaces?.size ?: 0) > 1) {
            listGen++
            flow.update { it.copy(space = null, listing = null, loading = false, error = null) }
            loadSpaces(reloadKept = true)
        }
    }

    private fun load(clear: Boolean, refreshing: Boolean) {
        val s = flow.value
        val space = s.space ?: return
        val folderId = s.trail.lastOrNull()?.id
        val gen = ++listGen
        flow.update {
            val keep = if (clear) null else it.listing
            it.copy(listing = keep, loading = keep == null, refreshing = refreshing && keep != null, error = null)
        }
        scope.launch {
            val result = call { backend.files(space.accountId, folderId) }
            if (gen != listGen) return@launch
            result.onSuccess { listing ->
                flow.update {
                    it.copy(
                        listing = listing,
                        trail = trailOf(listing, folderId, it.trail),
                        trailConfirmed = true,
                        loading = false,
                        refreshing = false,
                        error = null,
                        folderNames = it.folderNames + names(listing),
                    )
                }
            }.onFailure { e -> flow.update { it.copy(loading = false, refreshing = false, error = FilesWords.message(e)) } }
        }
    }

    private fun trailOf(listing: FileListing, folderId: String?, current: List<FileEntry>): List<FileEntry> {
        if (folderId == null) return emptyList()
        var path = listing.path
        val folder = listing.folder
        if (folder != null && path.lastOrNull()?.id != folder.id) path = path + folder
        return if (path.lastOrNull()?.id == folderId) path else current
    }

    private fun names(listing: FileListing): Map<String, String> {
        val map = HashMap<String, String>()
        for (folder in listing.path) map[folder.id] = folder.name
        listing.folder?.let { map[it.id] = it.name }
        for (entry in listing.entries) if (entry.kind == FileKind.FOLDER) map[entry.id] = entry.name
        return map
    }

    // ---- search

    fun openSearch() {
        if (flow.value.space == null) return
        flow.update { it.copy(query = "", results = null, searching = false, searchError = null) }
    }

    fun closeSearch() {
        searchGen++
        searchJob?.cancel()
        flow.update { it.copy(query = null, results = null, searching = false, searchError = null) }
    }

    /** The search field changed: the answer comes [searchDelayMillis] after the last key; a blank text clears the results at once. */
    fun setQuery(text: String) {
        val space = flow.value.space ?: return
        searchJob?.cancel()
        val gen = ++searchGen
        val blank = text.isBlank()
        flow.update { it.copy(query = text, results = if (blank) null else it.results, searching = !blank, searchError = null) }
        if (blank) return
        searchJob = scope.launch {
            delay(searchDelayMillis)
            val result = call { backend.search(space.accountId, text.trim()) }
            if (gen != searchGen) return@launch
            result.onSuccess { list ->
                flow.update { it.copy(results = list, searching = false, searchError = null) }
            }.onFailure { e -> flow.update { it.copy(searching = false, searchError = FilesWords.message(e)) } }
        }
    }

    // ---- opening and saving

    /** A tap on an entry: a folder opens, a file is downloaded and handed to a viewer, anything else cannot be opened here. */
    fun open(entry: FileEntry) {
        when (entry.kind) {
            FileKind.FOLDER -> openFolder(entry)
            FileKind.OTHER -> say(R.string.files_cannot_open)
            FileKind.FILE -> download(entry, saving = false, onFail = {}) { file ->
                channel.trySend(FilesEvent.View(file, FileRules.viewType(entry.mediaType, entry.name)))
            }
        }
    }

    /** The save picker made the document [target]: download and copy. */
    fun save(target: String) {
        val entry = pendingSave
        pendingSave = null
        if (entry == null) {
            // We forgot what this was for: do not leave an empty document behind.
            scope.launch { call { platform.discard(target) } }
            say(R.string.files_save_failed)
            return
        }
        download(entry, saving = true, onFail = { platform.discard(target) }) { file ->
            val copied = call { platform.copyTo(file, target) }
            if (copied.isSuccess) {
                say(R.string.files_saved)
            } else {
                call { platform.discard(target) }
                say(R.string.files_save_failed)
            }
        }
    }

    /** The save picker was left without a choice. */
    fun cancelSave() {
        pendingSave = null
    }

    private fun download(entry: FileEntry, saving: Boolean, onFail: () -> Unit, then: suspend (File) -> Unit) {
        val space = flow.value.space ?: return
        if (flow.value.opening != null) return
        val blobId = entry.blobId
        if (blobId == null) {
            say(R.string.files_failed)
            return
        }
        val stop = DownloadStop()
        openStop = stop
        flow.update { it.copy(opening = entry.name, openingToSave = saving, openingProgress = DownloadProgress(0, DownloadRules.expected(entry.size.toLong()))) }
        val gen = ++openGen
        openJob = scope.launch {
            try {
                val result = call {
                    val file = platform.cacheFile(space.accountId, entry)
                    var complete = true
                    if (!file.isFile) {
                        file.parentFile?.mkdirs()
                        complete = backend.download(space.accountId, blobId, file.absolutePath, stop) { done, total ->
                            if (gen == openGen) {
                                flow.update { now ->
                                    if (now.opening == null) now else now.copy(openingProgress = DownloadProgress(done, total ?: now.openingProgress?.total))
                                }
                            }
                        }
                    }
                    // Null: stopped by the user, nothing is left of the file.
                    if (complete) file else null
                }
                result.onSuccess { file -> if (file != null) then(file) else call(onFail) }.onFailure { e ->
                    call(onFail)
                    say(FilesWords.message(e))
                }
            } catch (e: CancellationException) {
                withContext(kotlinx.coroutines.NonCancellable) { withContext(io) { onFail() } }
                throw e
            } finally {
                // A cancelled download ends late (the core call cannot be interrupted): by then the window may
                // belong to the next file.
                if (gen == openGen) {
                    openStop = null
                    flow.update { it.copy(opening = null, openingToSave = false, openingProgress = null) }
                }
            }
        }
    }

    /** The user closed the "Opening" window: the download stops and the file is not opened. */
    fun cancelOpening() {
        openStop?.stop()
        openStop = null
        openJob?.cancel()
        openGen++
        flow.update { it.copy(opening = null, openingToSave = false, openingProgress = null) }
    }

    // ---- dialogs and changes

    fun dismissDialog() {
        moveGen++
        linksGen++
        flow.update { it.copy(dialog = null, dialogError = null) }
    }

    fun askNewFolder() = flow.update { if (it.mayAdd) it.copy(dialog = FilesDialog.NewFolder, dialogError = null) else it }

    fun askRename(entry: FileEntry) = flow.update { it.copy(dialog = FilesDialog.Rename(entry), dialogError = null) }

    fun askDelete(entry: FileEntry) = flow.update { it.copy(dialog = FilesDialog.Delete(entry), dialogError = null) }

    // ---- public links

    /** Opens the public links of [entry] and reads them; nothing happens where [FileRules.publicLinkAllowed] says no. */
    fun askPublicLinks(entry: FileEntry) {
        val limits = flow.value.publicLinkLimits ?: return
        if (!FileRules.publicLinkAllowed(entry, supported = true)) return
        val validity = FileRules.defaultValidity(limits)
        flow.update { it.copy(dialog = FilesDialog.PublicLinks(entry, validity), dialogError = null) }
        loadLinks(entry)
    }

    fun linksRetry() {
        val d = flow.value.dialog as? FilesDialog.PublicLinks ?: return
        flow.update { s -> s.copy(dialog = d.copy(links = null, error = null)) }
        loadLinks(d.entry)
    }

    private fun loadLinks(entry: FileEntry) {
        val gen = ++linksGen
        scope.launch {
            val result = call { backend.publicLinks(entry) }
            if (gen != linksGen) return@launch
            result.onSuccess { links -> updateLinks(entry) { it.copy(links = links, error = null) } }
                .onFailure { e -> updateLinks(entry) { it.copy(error = FilesWords.message(e)) } }
        }
    }

    /** Changes the open links dialog if it is still the one for [entry]. */
    private fun updateLinks(entry: FileEntry, change: (FilesDialog.PublicLinks) -> FilesDialog.PublicLinks) {
        flow.update { s ->
            val d = s.dialog
            if (d is FilesDialog.PublicLinks && d.entry.id == entry.id) s.copy(dialog = change(d)) else s
        }
    }

    fun chooseValidity(validity: LinkValidity) {
        val d = flow.value.dialog as? FilesDialog.PublicLinks ?: return
        updateLinks(d.entry) { it.copy(validity = validity) }
    }

    /** Makes a link with the chosen validity; it goes on top of the list and its address is handed to the screen to copy. */
    fun makeLink() {
        val d = flow.value.dialog as? FilesDialog.PublicLinks ?: return
        val limits = flow.value.publicLinkLimits ?: return
        val links = d.links ?: return
        if (d.working || FileRules.atLinkLimit(links, limits)) return
        val expiresIn = FileRules.expiresIn(d.validity, limits)
        updateLinks(d.entry) { it.copy(working = true, error = null) }
        scope.launch {
            val result = call { backend.publicLinkCreate(d.entry, expiresIn) }
            result.onSuccess { link ->
                updateLinks(d.entry) { it.copy(links = listOf(link) + it.links.orEmpty(), working = false, copiedId = link.id) }
                channel.trySend(FilesEvent.CopyLink(link.url))
            }.onFailure { e -> updateLinks(d.entry) { it.copy(working = false, error = FilesWords.message(e)) } }
        }
    }

    /** The screen put the address of link [id] on the clipboard. */
    fun linkCopied(id: String) {
        val d = flow.value.dialog as? FilesDialog.PublicLinks ?: return
        updateLinks(d.entry) { it.copy(copiedId = id) }
    }

    fun askRevoke(link: PublicLink) {
        val d = flow.value.dialog as? FilesDialog.PublicLinks ?: return
        updateLinks(d.entry) { it.copy(revoking = link, error = null) }
    }

    fun cancelRevoke() {
        val d = flow.value.dialog as? FilesDialog.PublicLinks ?: return
        updateLinks(d.entry) { it.copy(revoking = null) }
    }

    /** Revokes the link the user was asked about; it stops working at once on the server and leaves the list. */
    fun revokeLink() {
        val d = flow.value.dialog as? FilesDialog.PublicLinks ?: return
        val link = d.revoking ?: return
        if (d.working) return
        updateLinks(d.entry) { it.copy(working = true) }
        scope.launch {
            val result = call { backend.publicLinkRevoke(d.entry, link.id) }
            result.onSuccess { _ ->
                updateLinks(d.entry) { it.copy(links = it.links?.filter { l -> l.id != link.id }, revoking = null, working = false) }
            }.onFailure { e -> updateLinks(d.entry) { it.copy(revoking = null, working = false, error = FilesWords.message(e)) } }
        }
    }

    /** Opens the move dialog on the folder the entry is in (the top level for a search result). */
    fun askMove(entry: FileEntry) {
        val start = if (flow.value.query != null) emptyList() else flow.value.trail
        flow.update { it.copy(dialog = FilesDialog.Move(entry, start), dialogError = null) }
        moveLoad(entry, start)
    }

    fun moveOpen(folder: FileEntry) {
        val d = flow.value.dialog as? FilesDialog.Move ?: return
        moveLoad(d.entry, d.trail + folder)
    }

    fun moveUp() {
        val d = flow.value.dialog as? FilesDialog.Move ?: return
        if (d.trail.isNotEmpty()) moveLoad(d.entry, d.trail.dropLast(1))
    }

    fun moveRetry() {
        val d = flow.value.dialog as? FilesDialog.Move ?: return
        moveLoad(d.entry, d.trail)
    }

    private fun moveLoad(entry: FileEntry, trail: List<FileEntry>) {
        val space = flow.value.space ?: return
        val gen = ++moveGen
        flow.update { s -> if (s.dialog is FilesDialog.Move) s.copy(dialog = FilesDialog.Move(entry, trail)) else s }
        scope.launch {
            val result = call { backend.files(space.accountId, trail.lastOrNull()?.id) }
            if (gen != moveGen) return@launch
            result.onSuccess { listing ->
                flow.update { s ->
                    val d = s.dialog
                    if (d is FilesDialog.Move && d.entry.id == entry.id) {
                        s.copy(dialog = d.copy(trail = trailOf(listing, trail.lastOrNull()?.id, trail), listing = listing, error = null))
                    } else {
                        s
                    }
                }
            }.onFailure { e ->
                flow.update { s ->
                    val d = s.dialog
                    if (d is FilesDialog.Move && d.entry.id == entry.id) s.copy(dialog = d.copy(listing = null, error = FilesWords.message(e))) else s
                }
            }
        }
    }

    fun createFolder(name: String) {
        val clean = FileRules.cleanName(name)
        val parent = flow.value.trail.lastOrNull()?.id
        if (clean.isEmpty()) return
        change { account -> backend.createFolder(account, parent, clean) }
    }

    fun rename(entry: FileEntry, name: String) {
        val clean = FileRules.cleanName(name)
        if (clean.isEmpty()) return
        if (clean == entry.name) {
            dismissDialog()
            return
        }
        change { account -> backend.rename(account, entry.id, clean) }
    }

    fun delete(entry: FileEntry) = change { account -> backend.delete(account, entry.id) }

    fun moveHere() {
        val d = flow.value.dialog as? FilesDialog.Move ?: return
        val listing = d.listing ?: return
        if (!FileRules.canMoveHere(d.entry, d.trail, listing.mayAdd)) return
        val destination = d.trail.lastOrNull()?.id
        change { account -> backend.move(account, d.entry.id, destination) }
    }

    /** Runs a change; on success the dialog closes and the listing is read again, on failure the dialog stays and says why. */
    private fun change(work: (account: String?) -> Unit) {
        val space = flow.value.space ?: return
        if (flow.value.busy) return
        flow.update { it.copy(busy = true, dialogError = null) }
        scope.launch {
            val result = call { work(space.accountId) }
            result.onSuccess {
                moveGen++
                flow.update { it.copy(busy = false, dialog = null, dialogError = null) }
                reloadAfterChange()
            }.onFailure { e ->
                val message = FilesWords.message(e)
                if (flow.value.dialog == null) say(message)
                flow.update { it.copy(busy = false, dialogError = message) }
            }
        }
    }

    private fun reloadAfterChange() {
        if (flow.value.space != null) load(clear = false, refreshing = false)
        val q = flow.value.query
        if (q != null && q.isNotBlank()) setQuery(q)
    }

    // ---- upload

    /** Sends the picked documents (content URIs as text) into the folder shown now, one after the other. */
    fun upload(items: List<String>) {
        val s = flow.value
        val space = s.space ?: return
        if (items.isEmpty()) return
        if (s.upload != null) {
            say(R.string.files_upload_running)
            return
        }
        val parentId = s.trail.lastOrNull()?.id
        stopRequested = false
        flow.update { it.copy(upload = UploadState(1, items.size, "")) }
        uploadJob = scope.launch {
            var queue = UploadQueue(items)
            var tally = UploadTally()
            try {
                while (true) {
                    if (stopRequested) queue = queue.stop()
                    val item = queue.current ?: break
                    flow.update { it.copy(upload = UploadState(queue.position, queue.total, "", stopping = stopRequested)) }
                    val step = sendOne(space, parentId, item, queue)
                    tally = tally.record(step.outcome)
                    if (step.stopAll) queue = queue.stop()
                    queue = queue.advance()
                    if (flow.value.space?.accountId == space.accountId && flow.value.trail.lastOrNull()?.id == parentId) {
                        reloadAfterChange()
                    }
                }
            } finally {
                flow.update { it.copy(upload = null) }
                conflictAnswer = null
            }
            channel.trySend(FilesEvent.UploadDone(queue.total, tally))
            if (flow.value.space?.accountId == space.accountId && flow.value.trail.lastOrNull()?.id == parentId) reloadAfterChange()
        }
    }

    /** Lets the file being sent finish and sends no more. */
    fun stopUpload() {
        stopRequested = true
        flow.update { s -> s.upload?.let { s.copy(upload = it.copy(stopping = true)) } ?: s }
        // A question waiting for an answer is answered with Skip, so the stop is not held up by it.
        conflictAnswer?.complete(ConflictChoice.Skip)
    }

    /** The answer to the "already there" question. */
    fun resolveConflict(choice: ConflictChoice) {
        conflictAnswer?.complete(choice)
    }

    private class Step(val outcome: UploadOutcome, val stopAll: Boolean = false)

    private fun setUploadName(name: String) = flow.update { s -> s.upload?.let { s.copy(upload = it.copy(name = name)) } ?: s }

    private suspend fun sendOne(space: FileSpace, parentId: String?, item: String, queue: UploadQueue<String>): Step {
        val info = call { platform.describe(item) }.getOrElse {
            say(R.string.files_unreadable_generic)
            return Step(UploadOutcome.Failed)
        }
        setUploadName(info.name)
        val limit = space.maxUpload
        if (info.size != null && FileRules.tooLarge(info.size, limit)) {
            channel.trySend(FilesEvent.TooLarge(info.name, info.size, limit))
            return Step(UploadOutcome.Failed)
        }
        // What is in the folder now decides whether the name is taken.
        val listing = call { backend.files(space.accountId, parentId) }.getOrElse { e ->
            say(FilesWords.message(e))
            // No server, no session or no such folder any more: every file after this one would fail the same way.
            return Step(
                UploadOutcome.Failed,
                stopAll = e is CoreException.SignInNeeded || e is CoreException.Temporary || e is CoreException.Invalid,
            )
        }
        var name = info.name
        var replace = false
        val existing = listing.entries.firstOrNull { it.name == name }
        if (existing != null) {
            val answer = CompletableDeferred<ConflictChoice>()
            conflictAnswer = answer
            flow.update { s -> s.upload?.let { s.copy(upload = it.copy(conflict = UploadConflict(name, existing))) } ?: s }
            val choice = if (stopRequested) ConflictChoice.Skip else answer.await()
            conflictAnswer = null
            flow.update { s -> s.upload?.let { s.copy(upload = it.copy(conflict = null)) } ?: s }
            when (choice) {
                ConflictChoice.Skip -> return Step(UploadOutcome.Skipped)
                ConflictChoice.Replace -> replace = true
                ConflictChoice.KeepBoth -> name = FileRules.freeName(name, listing.entries.map { it.name })
            }
        }
        // The copy, the upload and the removal of the copy are one block that runs to its end on the IO thread even if
        // the job is cancelled meanwhile: a copy whose handle is lost in a cancel would stay in the cache.
        return withContext(io) { copyAndSend(space, parentId, item, name, replace) }
    }

    /** Blocks. Copies the picked document, sends it and removes the copy, whatever happens. */
    private fun copyAndSend(space: FileSpace, parentId: String?, item: String, name: String, replace: Boolean): Step {
        val picked = try {
            platform.copyForUpload(item, name)
        } catch (e: Exception) {
            if (e is IOException) channel.trySend(FilesEvent.Unreadable(name)) else say(R.string.files_failed)
            return Step(UploadOutcome.Failed)
        }
        try {
            if (FileRules.tooLarge(picked.size, space.maxUpload)) {
                channel.trySend(FilesEvent.TooLarge(name, picked.size, space.maxUpload))
                return Step(UploadOutcome.Failed)
            }
            return try {
                backend.upload(space.accountId, parentId, name, picked.mediaType, picked.path, replace)
                Step(UploadOutcome.Uploaded)
            } catch (e: Exception) {
                say(FilesWords.message(e))
                // "No more uploads for now": the rest would be turned away too.
                Step(UploadOutcome.Failed, stopAll = e is CoreException.Temporary || e is CoreException.SignInNeeded)
            }
        } finally {
            picked.cleanup()
        }
    }
}
