package ovh.kweb.mail.ui.files

import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind
import uniffi.kweb_core.FileListing
import uniffi.kweb_core.FileSpace

private val own = FileSpace(accountId = null, name = "", mayAddAtTop = true, maxUpload = 1000uL)
private val shared = FileSpace(accountId = "x", name = "boss@example.org", mayAddAtTop = false, maxUpload = 1000uL)

private fun folder(id: String, name: String, parent: String? = null) = entry(id = id, name = name, kind = FileKind.FOLDER, parentId = parent)

private fun listing(folder: FileEntry?, path: List<FileEntry>, vararg entries: FileEntry, mayAdd: Boolean = true) =
    FileListing(folder = folder, path = path, entries = entries.toList(), mayAdd = mayAdd)

private class FakeBackend : FilesBackend {
    var spaces: List<FileSpace> = listOf(own)
    val listings = HashMap<String?, FileListing>()
    val calls = CopyOnWriteArrayList<String>()
    var gate: ((String?) -> Unit)? = null
    var failRename: Exception? = null
    var failUpload: ((String) -> Exception?)? = null

    override fun spaces() = spaces

    override fun files(account: String?, folderId: String?): FileListing {
        gate?.invoke(folderId)
        calls += "files:$folderId"
        return listings[folderId] ?: throw CoreException.Invalid("folder is gone")
    }

    var found: List<FileEntry> = emptyList()
    var downloadGate: ((String) -> Unit)? = null

    override fun search(account: String?, text: String) = found
    override fun createFolder(account: String?, parentId: String?, name: String) = folder("new", name, parentId)

    override fun upload(account: String?, parentId: String?, name: String, mediaType: String, sourcePath: String, replace: Boolean): FileEntry {
        failUpload?.invoke(name)?.let { throw it }
        calls += "upload:$name:replace=$replace"
        return entry(name = name)
    }

    override fun rename(account: String?, id: String, name: String) {
        failRename?.let { throw it }
        calls += "rename:$id:$name"
    }

    override fun move(account: String?, id: String, parentId: String?) {
        calls += "move:$id:$parentId"
    }

    override fun delete(account: String?, id: String) {
        calls += "delete:$id"
    }

    override fun download(account: String?, blobId: String, destPath: String) {
        downloadGate?.invoke(blobId)
        File(destPath).writeText("x")
        calls += "download:$blobId"
    }
}

/** A picked document is described as "name|size". */
private class FakePlatform(val dir: File) : FilesPlatform {
    val cleaned = CopyOnWriteArrayList<String>()
    val swept = java.util.concurrent.atomic.AtomicInteger()
    var copyGate: (() -> Unit)? = null
    override fun cacheFile(account: String?, entry: FileEntry) = File(dir, "cache/${entry.id}/${entry.name}")
    override fun describe(item: String): UploadInfo = UploadInfo(item.substringBefore('|'), item.substringAfter('|').toLongOrNull())
    override fun sweepLeftovers() {
        swept.incrementAndGet()
    }
    override fun copyForUpload(item: String, name: String): PickedUpload {
        copyGate?.invoke()
        val file = File(dir, "out/$name")
        file.parentFile?.mkdirs()
        file.writeText("data")
        return PickedUpload(file.absolutePath, "text/plain", describe(item).size ?: 0L) {
            cleaned += name
            file.delete()
        }
    }
    override fun copyTo(source: File, target: String) = Unit
    override fun discard(target: String) = Unit
}

class FilesControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeBackend
    private lateinit var platform: FakePlatform
    private lateinit var c: FilesController

    @Before fun setUp() {
        backend = FakeBackend()
        platform = FakePlatform(kotlin.io.path.createTempDirectory("files-test").toFile())
        c = FilesController(scope, pool.asCoroutineDispatcher(), backend, platform, searchDelayMillis = 0)
        backend.listings[null] = listing(null, emptyList(), folder("a", "A"), folder("b", "B"), entry(id = "f", name = "f.txt"))
        backend.listings["a"] = listing(folder("a", "A"), listOf(folder("a", "A")), entry(id = "a1", name = "one.txt", parentId = "a"))
        backend.listings["b"] = listing(folder("b", "B"), listOf(folder("b", "B")))
    }

    @After fun tearDown() {
        scope.cancel()
        pool.shutdownNow()
        platform.dir.deleteRecursively()
    }

    private fun await(what: String = "condition", cond: () -> Boolean) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!cond()) {
            if (System.nanoTime() > end) throw AssertionError("timed out waiting for $what; state ${c.state.value}")
            Thread.sleep(5)
        }
    }

    private fun shown() {
        c.onShown()
        await("top listing") { c.state.value.listing != null }
    }

    @Test fun onePlaceIsOpenedAtOnce() {
        shown()
        val s = c.state.value
        assertEquals(own, s.space)
        assertEquals(listOf("A", "B", "f.txt"), s.listing!!.entries.map { it.name })
        assertTrue(!s.canGoUp)
    }

    @Test fun severalPlacesStartWithTheChoice() {
        backend.spaces = listOf(own, shared)
        c.onShown()
        await("spaces") { c.state.value.spaces != null }
        assertTrue(c.state.value.atSpaceChoice)
        assertNull(c.state.value.listing)
        c.selectSpace(shared)
        await("listing") { c.state.value.listing != null }
        assertTrue(c.state.value.canGoUp)
        c.up()
        assertTrue(c.state.value.atSpaceChoice)
    }

    @Test fun noPlacesMeansNoStorage() {
        backend.spaces = emptyList()
        c.onShown()
        await("spaces") { c.state.value.spaces != null }
        assertEquals(emptyList<FileSpace>(), c.state.value.spaces)
        assertNull(c.state.value.space)
        assertTrue(!c.state.value.atSpaceChoice)
    }

    @Test fun openingAFolderAndGoingUp() {
        shown()
        c.open(folder("a", "A"))
        await("folder") { c.state.value.listing?.folder?.id == "a" }
        assertEquals(listOf("a"), c.state.value.trail.map { it.id })
        c.up()
        await("top") { c.state.value.listing?.folder == null && c.state.value.listing != null }
        assertEquals(emptyList<FileEntry>(), c.state.value.trail)
    }

    @Test fun aListingThatArrivesLateDoesNotReplaceANewerOne() {
        shown()
        val release = CountDownLatch(1)
        val started = CountDownLatch(1)
        backend.gate = { id -> if (id == "a") { started.countDown(); release.await(5, TimeUnit.SECONDS) } }
        c.open(folder("a", "A"))
        assertTrue(started.await(5, TimeUnit.SECONDS))
        // The user moves on to the top level and then B while A is still on its way.
        backend.gate = null
        c.up()
        await("top") { c.state.value.listing != null && c.state.value.listing!!.folder == null }
        c.open(folder("b", "B"))
        await("B") { c.state.value.listing?.folder?.id == "b" }
        release.countDown()
        Thread.sleep(150)
        assertEquals("b", c.state.value.listing?.folder?.id)
        assertEquals(listOf("b"), c.state.value.trail.map { it.id })
    }

    @Test fun aFailedListingSaysWhyAndRetryWorks() {
        shown()
        c.open(folder("gone", "Gone"))
        await("error") { c.state.value.error != null }
        assertNull(c.state.value.listing)
        backend.listings["gone"] = listing(folder("gone", "Gone"), listOf(folder("gone", "Gone")))
        c.refresh()
        await("listing") { c.state.value.listing != null }
        assertNull(c.state.value.error)
    }

    @Test fun renameChangesAndReloads() {
        shown()
        val f = c.state.value.listing!!.entries.last()
        c.askRename(f)
        c.rename(f, " g.txt ")
        await("dialog gone") { c.state.value.dialog == null && !c.state.value.busy }
        assertTrue("rename:f:g.txt" in backend.calls)
        assertEquals(2, backend.calls.count { it == "files:null" })
    }

    @Test fun aRefusedRenameKeepsTheDialogOpenWithTheReason() {
        shown()
        val f = c.state.value.listing!!.entries.last()
        backend.failRename = CoreException.Invalid("A file with that name is already there.")
        c.askRename(f)
        c.rename(f, "g.txt")
        await("error") { c.state.value.dialogError != null }
        assertTrue(c.state.value.dialog is FilesDialog.Rename)
        assertTrue(!c.state.value.busy)
    }

    @Test fun deleteAsksTheCoreAndReloads() {
        shown()
        val f = c.state.value.listing!!.entries.last()
        c.askDelete(f)
        c.delete(f)
        await("done") { c.state.value.dialog == null && !c.state.value.busy }
        assertTrue("delete:f" in backend.calls)
    }

    @Test fun theMoveDialogBrowsesAndMovesHere() {
        shown()
        val f = c.state.value.listing!!.entries.last()
        c.askMove(f)
        await("move listing") { (c.state.value.dialog as? FilesDialog.Move)?.listing != null }
        c.moveOpen(folder("a", "A"))
        await("A in dialog") { (c.state.value.dialog as? FilesDialog.Move)?.listing?.folder?.id == "a" }
        c.moveHere()
        await("closed") { c.state.value.dialog == null && !c.state.value.busy }
        assertTrue("move:f:a" in backend.calls)
    }

    @Test fun moveHereWhereTheItemAlreadyIsDoesNothing() {
        shown()
        val f = c.state.value.listing!!.entries.last()
        c.askMove(f)
        await("move listing") { (c.state.value.dialog as? FilesDialog.Move)?.listing != null }
        c.moveHere()
        Thread.sleep(100)
        assertTrue(backend.calls.none { it.startsWith("move") })
    }

    @Test fun filesAreUploadedOneAfterTheOtherAndTheCopiesAreRemoved() {
        shown()
        c.upload(listOf("p.txt|10", "q.txt|20"))
        await("done") { c.state.value.upload == null && backend.calls.count { it.startsWith("upload") } == 2 }
        assertEquals(listOf("upload:p.txt:replace=false", "upload:q.txt:replace=false"), backend.calls.filter { it.startsWith("upload") })
        assertEquals(listOf("p.txt", "q.txt"), platform.cleaned.toList())
    }

    @Test fun aFileOverTheLimitIsNotCopiedOrSent() {
        shown()
        c.upload(listOf("big.bin|5000", "ok.txt|10"))
        await("done") { c.state.value.upload == null && backend.calls.any { it.startsWith("upload:ok") } }
        assertTrue(backend.calls.none { it.startsWith("upload:big") })
        assertEquals(listOf("ok.txt"), platform.cleaned.toList())
    }

    @Test fun aNameAlreadyThereAsksAndKeepBothUsesAFreeName() {
        backend.listings[null] = listing(null, emptyList(), entry(id = "f", name = "f.txt"), entry(id = "g", name = "f (2).txt"))
        shown()
        c.upload(listOf("f.txt|10"))
        await("question") { c.state.value.upload?.conflict != null }
        assertEquals("f.txt", c.state.value.upload!!.conflict!!.name)
        c.resolveConflict(ConflictChoice.KeepBoth)
        await("done") { c.state.value.upload == null }
        assertEquals(listOf("upload:f (3).txt:replace=false"), backend.calls.filter { it.startsWith("upload") })
    }

    @Test fun replaceUploadsAgainWithReplace() {
        shown()
        c.upload(listOf("f.txt|10"))
        await("question") { c.state.value.upload?.conflict != null }
        c.resolveConflict(ConflictChoice.Replace)
        await("done") { c.state.value.upload == null }
        assertEquals(listOf("upload:f.txt:replace=true"), backend.calls.filter { it.startsWith("upload") })
    }

    @Test fun skipSendsNothingForThatFileButGoesOn() {
        shown()
        c.upload(listOf("f.txt|10", "n.txt|10"))
        await("question") { c.state.value.upload?.conflict != null }
        c.resolveConflict(ConflictChoice.Skip)
        await("done") { c.state.value.upload == null && backend.calls.any { it.startsWith("upload:n") } }
        assertTrue(backend.calls.none { it.startsWith("upload:f.txt") })
    }

    @Test fun stoppingLetsTheCurrentFileFinishAndSendsNoMore() {
        shown()
        val gate = CountDownLatch(1)
        backend.failUpload = { gate.await(5, TimeUnit.SECONDS); null }
        c.upload(listOf("a1.txt|1", "a2.txt|1", "a3.txt|1"))
        await("first") { c.state.value.upload?.name == "a1.txt" }
        c.stopUpload()
        assertTrue(c.state.value.upload!!.stopping)
        gate.countDown()
        await("done") { c.state.value.upload == null }
        assertEquals(listOf("upload:a1.txt:replace=false"), backend.calls.filter { it.startsWith("upload") })
        assertEquals(listOf("a1.txt"), platform.cleaned.toList())
    }

    @Test fun noMoreUploadsForNowEndsTheQueueAndStillRemovesTheCopy() {
        shown()
        backend.failUpload = { CoreException.Temporary("Too many uploads. Try again in 200 seconds.") }
        c.upload(listOf("a1.txt|1", "a2.txt|1"))
        await("done") { c.state.value.upload == null }
        assertEquals(listOf("a1.txt"), platform.cleaned.toList())
        assertTrue(backend.calls.none { it.startsWith("upload") })
    }

    @Test fun downloadingAFileMakesItReadyToOpen() {
        shown()
        val f = c.state.value.listing!!.entries.last()
        c.open(f)
        await("download") { backend.calls.contains("download:blob-f") && c.state.value.opening == null }
        assertTrue(platform.cacheFile(null, f).isFile)
    }

    @Test fun anItemTheAppCannotOpenSaysSo() {
        shown()
        c.open(entry(id = "l", name = "link", kind = FileKind.OTHER))
        assertNull(c.state.value.opening)
        assertTrue(backend.calls.none { it.startsWith("download") })
        assertNotNull(c.state.value.listing)
    }

    @Test fun aCopyIsRemovedEvenWhenTheUploadIsCancelledWhileCopying() {
        shown()
        val inCopy = CountDownLatch(1)
        val release = CountDownLatch(1)
        platform.copyGate = { inCopy.countDown(); release.await(5, TimeUnit.SECONDS) }
        c.upload(listOf("p.txt|10"))
        assertTrue(inCopy.await(5, TimeUnit.SECONDS))
        c.reset()
        release.countDown()
        await("copy removed") { platform.cleaned.toList() == listOf("p.txt") }
    }

    @Test fun leftoverCopiesAreSweptAtStartAndOnSignOut() {
        await("sweep at start") { platform.swept.get() == 1 }
        c.reset()
        await("sweep at sign-out") { platform.swept.get() == 2 }
    }

    @Test fun aCancelledDownloadDoesNotCloseTheWindowOfTheNextFile() {
        shown()
        val latchA = CountDownLatch(1)
        val latchB = CountDownLatch(1)
        backend.downloadGate = { blob -> if (blob == "blob-fa") latchA.await(5, TimeUnit.SECONDS) else latchB.await(5, TimeUnit.SECONDS) }
        c.open(entry(id = "fa", name = "a.bin"))
        assertEquals("a.bin", c.state.value.opening)
        c.cancelOpening()
        assertNull(c.state.value.opening)
        c.open(entry(id = "fb", name = "b.bin"))
        assertEquals("b.bin", c.state.value.opening)
        latchA.countDown()
        Thread.sleep(200)
        assertEquals("b.bin", c.state.value.opening)
        latchB.countDown()
        await("B done") { c.state.value.opening == null }
        assertTrue(backend.calls.contains("download:blob-fb"))
    }

    @Test fun savingIsMarkedAsSavingNotOpening() {
        shown()
        val latch = CountDownLatch(1)
        backend.downloadGate = { latch.await(5, TimeUnit.SECONDS) }
        c.pendingSave = entry(id = "fa", name = "a.bin")
        c.save("content://target")
        assertTrue(c.state.value.openingToSave)
        latch.countDown()
        await("done") { c.state.value.opening == null }
        assertTrue(!c.state.value.openingToSave)
    }

    @Test fun placesAreAskedAgainSoANewShareAppears() {
        shown()
        assertEquals(listOf(own), c.state.value.spaces)
        backend.spaces = listOf(own, shared)
        c.refresh()
        await("two places") { c.state.value.spaces?.size == 2 }
        // The folder shown stays, and the way back to the choice is open now.
        assertEquals(own, c.state.value.space)
        assertNotNull(c.state.value.listing)
        assertTrue(c.state.value.canGoUp)
    }

    @Test fun anEmptyListOfPlacesIsAskedAgainWhenTheScreenComesBack() {
        backend.spaces = emptyList()
        c.onShown()
        await("empty") { c.state.value.spaces != null }
        backend.spaces = listOf(own)
        c.onShown()
        await("listing") { c.state.value.listing != null }
        assertEquals(own, c.state.value.space)
    }

    @Test fun aFailedReadOfPlacesLaterKeepsWhatIsShown() {
        shown()
        backend.spaces = emptyList()
        val failing = object : FilesBackend by backend {
            override fun spaces(): List<FileSpace> = throw CoreException.Temporary("offline")
        }
        val c2 = FilesController(scope, pool.asCoroutineDispatcher(), failing, platform, searchDelayMillis = 0)
        c2.onShown()
        await("error") { c2.state.value.error != null }
        assertNull(c2.state.value.spaces)
    }

    @Test fun aServerThatCannotBeReachedStopsTheQueueAtOnceWithOneMessage() {
        shown()
        val events = CopyOnWriteArrayList<FilesEvent>()
        scope.launch { c.events.collect { events += it } }
        backend.listings.clear()
        c.upload(listOf("a1.txt|1", "a2.txt|1", "a3.txt|1"))
        await("done") { events.any { it is FilesEvent.UploadDone } }
        assertEquals(1, events.count { it is FilesEvent.Say })
        assertTrue(backend.calls.none { it.startsWith("upload") })
    }

    @Test fun aFolderFromASearchIsNotTrustedUntilItsListingArrives() {
        shown()
        val sub = folder("sub", "Sub", parent = "a")
        backend.found = listOf(sub)
        c.openSearch()
        c.setQuery("sub")
        await("results") { c.state.value.results != null }
        c.open(sub)
        await("error") { c.state.value.error != null }
        assertTrue(!c.state.value.trailConfirmed)
        // Back goes to the search, not to the top level.
        c.up()
        assertEquals("sub", c.state.value.query)
        assertEquals(emptyList<FileEntry>(), c.state.value.trail)
        await("results again") { c.state.value.results != null }
    }

    @Test fun aFolderFromASearchIsConfirmedByItsListing() {
        shown()
        val sub = folder("sub", "Sub", parent = "a")
        backend.listings["sub"] = listing(sub, listOf(folder("a", "A"), sub))
        backend.found = listOf(sub)
        c.openSearch()
        c.setQuery("sub")
        await("results") { c.state.value.results != null }
        c.open(sub)
        await("listing") { c.state.value.listing?.folder?.id == "sub" }
        assertTrue(c.state.value.trailConfirmed)
        assertEquals(listOf("a", "sub"), c.state.value.trail.map { it.id })
    }

    @Test fun signingOutForgetsEverything() {
        shown()
        c.reset()
        assertEquals(FilesState(), c.state.value)
    }
}
