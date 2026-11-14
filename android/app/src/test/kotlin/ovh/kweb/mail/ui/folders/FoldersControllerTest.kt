package ovh.kweb.mail.ui.folders

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.Exception as CoreException

private class FakeBackend : FoldersBackend {
    var boxes: List<Mailbox> = listOf(
        folder("in", role = "inbox", notify = true),
        folder("tr", role = "trash"),
        folder("ar", role = "archive"),
        folder("work", total = 4u),
        folder("sub", parent = "work"),
        folder("empty"),
    )
    val calls = CopyOnWriteArrayList<String>()
    var failWith: Exception? = null
    var failRead: Exception? = null
    var gate: (() -> Unit)? = null

    override fun mailboxes(): List<Mailbox> {
        val before = boxes
        gate?.invoke()
        calls += "read"
        failRead?.let { throw it }
        return before
    }

    private fun change(call: String) {
        calls += call
        failWith?.let { throw it }
    }

    override fun setNotify(mailboxId: String, notify: Boolean) {
        change("notify:$mailboxId:$notify")
        boxes = boxes.map { if (it.id == mailboxId) it.copy(notify = notify) else it }
    }

    override fun create(name: String, parentId: String?): Mailbox {
        change("create:$name:$parentId")
        val made = folder("new-$name", name, parentId)
        boxes = boxes + made
        return made
    }

    override fun rename(mailboxId: String, name: String) {
        change("rename:$mailboxId:$name")
        boxes = boxes.map { if (it.id == mailboxId) it.copy(name = name) else it }
    }

    override fun move(mailboxId: String, parentId: String?) {
        change("move:$mailboxId:$parentId")
        boxes = boxes.map { if (it.id == mailboxId) it.copy(parentId = parentId) else it }
    }

    override fun delete(mailboxId: String, withMail: Boolean) {
        change("delete:$mailboxId:$withMail")
        boxes = boxes.filter { it.id != mailboxId }
    }
}

class FoldersControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeBackend
    private lateinit var c: FoldersController

    @Before fun setUp() {
        backend = FakeBackend()
        c = FoldersController(scope, pool.asCoroutineDispatcher(), backend)
    }

    @After fun tearDown() {
        pool.shutdownNow()
    }

    private fun waitFor(what: String = "state", check: () -> Boolean) {
        val end = System.currentTimeMillis() + 5000
        while (!check()) {
            if (System.currentTimeMillis() > end) throw AssertionError("timed out waiting for $what: ${c.state.value}")
            Thread.sleep(5)
        }
    }

    private fun ready(folderId: String? = null, action: FolderAction? = null) {
        c.start(folderId, action)
        waitFor("ready") { c.state.value.phase == FoldersPhase.READY }
    }

    private fun idle() = waitFor("idle") { !c.state.value.busy }

    private val changes get() = backend.calls.filter { it != "read" }

    @Test fun loadsTheFolders() {
        ready()
        assertEquals(6, c.state.value.mailboxes.size)
    }

    @Test fun aFailedFirstReadCanBeRetried() {
        backend.failRead = java.io.IOException("x")
        c.start(null, null)
        waitFor("failed") { c.state.value.phase == FoldersPhase.FAILED }
        backend.failRead = null
        c.load()
        waitFor("ready") { c.state.value.phase == FoldersPhase.READY }
    }

    @Test fun anActionAskedForBeforeAFailedFirstReadOpensAfterTheRetry() {
        backend.failRead = java.io.IOException("x")
        c.start("empty", FolderAction.DELETE)
        waitFor("failed") { c.state.value.phase == FoldersPhase.FAILED }
        assertNull(c.state.value.dialog)
        backend.failRead = null
        c.load()
        waitFor("dialog") { c.state.value.dialog != null }
        assertEquals(FolderDialog.Delete("empty"), c.state.value.dialog)
    }

    @Test fun aFailedReadDoesNotPutItsMessageInTheNameField() {
        ready()
        c.askNew(null)
        c.setName("x")
        backend.failRead = java.io.IOException("x")
        c.refresh()
        waitFor("read") { backend.calls.count { it == "read" } >= 2 }
        Thread.sleep(200)
        assertNull(c.state.value.error)
    }

    @Test fun aRefusedNameKeepsTheCoresWordsWhenTheReadAfterItFails() {
        ready()
        c.askNew(null)
        c.setName("work")
        backend.failWith = CoreException.Invalid("A folder with that name is already there.")
        backend.failRead = java.io.IOException("x")
        c.confirmName()
        idle()
        assertEquals(UiMessage.Text("A folder with that name is already there."), c.state.value.error)
    }

    @Test fun aFailedLaterReadKeepsTheFoldersAndSaysSo() {
        ready()
        backend.failRead = java.io.IOException("x")
        c.refresh()
        waitFor("error") { c.state.value.error != null }
        assertEquals(6, c.state.value.mailboxes.size)
        assertEquals(FoldersPhase.READY, c.state.value.phase)
    }

    @Test fun startingWithAFolderAndAnActionOpensItsDialog() {
        ready("work", FolderAction.RENAME)
        waitFor("dialog") { c.state.value.dialog != null }
        assertEquals(FolderDialog.Name(NameMode.RENAME, "work", "work"), c.state.value.dialog)
    }

    @Test fun startOnlyHappensOnce() {
        ready()
        c.start("work", FolderAction.DELETE)
        Thread.sleep(50)
        assertNull(c.state.value.dialog)
    }

    @Test fun theBellSwitchesAFolderAndTheCoreIsAskedOnce() {
        ready()
        c.setNotify("work", true)
        idle()
        assertEquals(listOf("notify:work:true"), changes)
        assertTrue(c.state.value.mailboxes.first { it.id == "work" }.notify)
    }

    @Test fun theBellOfTheInboxAndOfSystemFoldersIsNotSent() {
        ready()
        c.setNotify("in", false)
        c.setNotify("tr", true)
        c.setNotify("nothing", true)
        Thread.sleep(50)
        assertEquals(emptyList<String>(), changes)
    }

    @Test fun aRefusedBellShowsTheCoresWordsAndReadsAgain() {
        ready()
        backend.failWith = CoreException.Invalid("Drafts cannot notify.")
        c.setNotify("work", true)
        idle()
        assertEquals(UiMessage.Text("Drafts cannot notify."), c.state.value.error)
        assertFalse(c.state.value.mailboxes.first { it.id == "work" }.notify)
    }

    @Test fun anotherFailureGetsTheAppsOwnWords() {
        ready()
        backend.failWith = java.io.IOException("x")
        c.setNotify("work", true)
        idle()
        assertEquals(UiMessage.Res(R.string.folders_notify_failed), c.state.value.error)
    }

    @Test fun aNewTopLevelFolderAsksForAName() {
        ready()
        c.askNew(null)
        assertEquals(FolderDialog.Name(NameMode.NEW, null, ""), c.state.value.dialog)
        c.setName("  Bills ")
        c.confirmName()
        idle()
        assertEquals(listOf("create:Bills:null"), changes)
        assertNull(c.state.value.dialog)
        assertTrue(c.state.value.mailboxes.any { it.name == "Bills" })
    }

    @Test fun aBlankNameSendsNothing() {
        ready()
        c.askNew("work")
        c.setName("   ")
        c.confirmName()
        Thread.sleep(50)
        assertEquals(emptyList<String>(), changes)
        assertNotNull(c.state.value.dialog)
    }

    @Test fun newFolderInsideIsRefusedForDraftsTrashAndJunk() {
        backend.boxes = backend.boxes + folder("dr", role = "drafts") + folder("ju", role = "junk")
        ready()
        for (id in listOf("dr", "tr", "ju")) {
            c.askNew(id)
            assertNull(c.state.value.dialog)
        }
        c.askNew("in")
        assertNotNull(c.state.value.dialog)
    }

    @Test fun aRefusedNameStaysInTheDialogWithTheCoresWordsAndTypingClearsIt() {
        ready()
        backend.failWith = CoreException.Invalid("A folder with that name is already there.")
        c.askNew(null)
        c.setName("work")
        c.confirmName()
        idle()
        assertEquals(UiMessage.Text("A folder with that name is already there."), c.state.value.error)
        assertEquals(FolderDialog.Name(NameMode.NEW, null, "work"), c.state.value.dialog)
        c.setName("work2")
        assertNull(c.state.value.error)
        assertEquals("work2", (c.state.value.dialog as FolderDialog.Name).text)
    }

    @Test fun renamingStartsFromTheCurrentNameAndRolesCannotBeRenamed() {
        ready()
        c.askRename("ar")
        assertNull(c.state.value.dialog)
        c.askRename("empty")
        assertEquals(FolderDialog.Name(NameMode.RENAME, "empty", "empty"), c.state.value.dialog)
        c.setName("Old")
        c.confirmName()
        idle()
        assertEquals(listOf("rename:empty:Old"), changes)
        assertEquals("Old", c.state.value.mailboxes.first { it.id == "empty" }.name)
    }

    @Test fun movingGoesToTheChosenParentOrTheTopLevel() {
        ready()
        c.askMove("sub")
        assertEquals(FolderDialog.Move("sub"), c.state.value.dialog)
        c.confirmMove(null)
        idle()
        assertEquals(listOf("move:sub:null"), changes)
        assertNull(c.state.value.mailboxes.first { it.id == "sub" }.parentId)
        assertNull(c.state.value.dialog)
        c.askMove("sub")
        c.confirmMove("empty")
        idle()
        assertEquals("empty", c.state.value.mailboxes.first { it.id == "sub" }.parentId)
    }

    @Test fun rolesCannotBeMovedOrDeleted() {
        ready()
        c.askMove("tr")
        assertNull(c.state.value.dialog)
        c.askDelete("in")
        assertNull(c.state.value.dialog)
    }

    @Test fun aFolderWithFoldersInsideIsNotDeletedAndSaysSo() {
        ready()
        c.askDelete("work")
        c.confirmDelete()
        Thread.sleep(50)
        assertEquals(emptyList<String>(), changes)
        assertNull(c.state.value.dialog)
    }

    @Test fun aFolderWithMailIsDeletedWithMail() {
        backend.boxes = backend.boxes.filter { it.id != "sub" }
        ready()
        c.askDelete("work")
        c.confirmDelete()
        idle()
        assertEquals(listOf("delete:work:true"), changes)
        assertTrue(c.state.value.mailboxes.none { it.id == "work" })
    }

    @Test fun anEmptyFolderIsDeletedWithoutMail() {
        ready()
        c.askDelete("empty")
        c.confirmDelete()
        idle()
        assertEquals(listOf("delete:empty:false"), changes)
    }

    @Test fun aRefusedDeleteShowsTheCoresWordsOnTheScreen() {
        ready()
        backend.failWith = CoreException.Invalid("Move the mail out first.")
        c.askDelete("empty")
        c.confirmDelete()
        idle()
        assertEquals(UiMessage.Text("Move the mail out first."), c.state.value.error)
        assertNull(c.state.value.dialog)
    }

    @Test fun aSecondChangeWhileOneIsOnItsWayIsRefused() {
        ready()
        val release = CountDownLatch(1)
        backend.gate = { }
        val slow = object : FoldersBackend by backend {
            override fun setNotify(mailboxId: String, notify: Boolean) {
                release.await(5, TimeUnit.SECONDS)
                backend.setNotify(mailboxId, notify)
            }
        }
        c = FoldersController(scope, pool.asCoroutineDispatcher(), slow)
        c.start(null, null)
        waitFor("ready") { c.state.value.phase == FoldersPhase.READY }
        c.setNotify("work", true)
        assertTrue(c.state.value.busy)
        c.setNotify("ar", true)
        c.askNew(null)
        assertNull(c.state.value.dialog)
        release.countDown()
        idle()
        assertEquals(listOf("notify:work:true"), changes)
    }

    @Test fun aLateReadDoesNotReplaceANewerOne() {
        ready()
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        // The first refresh is slow and sees the old list; a change and its read are done before it answers.
        val first = booleanArrayOf(true)
        backend.gate = {
            if (first[0]) {
                first[0] = false
                entered.countDown()
                release.await(5, TimeUnit.SECONDS)
            }
        }
        c.load()
        assertTrue(entered.await(5, TimeUnit.SECONDS))
        c.askRename("empty")
        c.setName("Newer")
        c.confirmName()
        waitFor("renamed") { c.state.value.mailboxes.any { it.name == "Newer" } }
        release.countDown()
        Thread.sleep(100)
        assertTrue(c.state.value.mailboxes.any { it.name == "Newer" })
        assertFalse(c.state.value.mailboxes.any { it.name == "empty" })
    }

    @Test fun aFreshReadClosesADialogAboutAFolderThatIsGone() {
        ready()
        c.askDelete("empty")
        backend.boxes = backend.boxes.filter { it.id != "empty" }
        c.refresh()
        waitFor("closed") { c.state.value.dialog == null }
    }

    @Test fun whatWasTypedSurvivesARefresh() {
        ready()
        c.askNew(null)
        c.setName("Half")
        c.refresh()
        waitFor("read twice") { backend.calls.count { it == "read" } >= 2 }
        Thread.sleep(50)
        assertEquals("Half", (c.state.value.dialog as FolderDialog.Name).text)
    }

    @Test fun resetForgetsEverything() {
        ready()
        c.askNew(null)
        c.reset()
        assertEquals(FoldersState(), c.state.value)
    }
}
