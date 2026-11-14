package ovh.kweb.mail.ui.sending

import java.io.File
import java.nio.file.Files
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.compose.PickedFile
import uniffi.kweb_core.Draft
import uniffi.kweb_core.Exception as CoreException

private fun draft(body: String = "", subject: String = "") = Draft("d1", "i1", emptyList(), emptyList(), emptyList(), subject, body, emptyList(), null, false, null, null, 0L)

private class FakeBackend(var started: Draft = draft()) : ShareBackend {
    val calls = ArrayList<String>()
    val saved = ArrayList<Draft>()
    var failNew = false
    var failSave = false
    var refuse = setOf<String>()
    var breakAttach = setOf<String>()

    override fun newDraft(): Draft {
        calls += "new"
        if (failNew) throw CoreException.Temporary("offline")
        return started
    }

    override fun saveDraft(draft: Draft): Draft {
        calls += "save"
        if (failSave) throw CoreException.Store("disk")
        saved += draft
        return draft
    }

    override fun addAttachment(draftId: String, path: String, name: String, mimeType: String): Draft {
        calls += "attach:$name"
        // The file must be there while the core reads it.
        assertTrue(File(path).exists())
        if (name in refuse) throw CoreException.Invalid("too large")
        if (name in breakAttach) throw CoreException.Temporary("network")
        return started
    }

    override fun discardDraft(draftId: String) {
        calls += "discard"
    }
}

class IncomingShareIntakeTest {
    private lateinit var root: File
    private val made = ArrayList<PickedFile>()

    @Before fun setUp() {
        root = Files.createTempDirectory("share-test").toFile()
    }

    @After fun tearDown() {
        root.deleteRecursively()
    }

    private val files = object : ShareFiles {
        override fun copy(uri: String): PickedFile {
            if (uri.contains("unreadable")) throw java.io.IOException("gone")
            val dir = File(root, "c${made.size}").also { it.mkdirs() }
            val name = uri.substringAfterLast('/')
            val f = File(dir, name).also { it.writeText("x") }
            return PickedFile(f, name, "image/png").also { made += it }
        }
    }

    private fun content(vararg uris: String, text: String = "", subject: String = "", skipped: Int = 0) = SharedContent(subject, text, uris.toList(), skipped)

    @Test fun textAndSubjectAreSavedInTheDraft() {
        val backend = FakeBackend()
        val out = IncomingShareIntake(backend, files).run(content(text = "a link", subject = "Title"))
        assertEquals(ShareOutcome.Ready("d1", 0, 0), out)
        assertEquals("a link", backend.saved.single().body)
        assertEquals("Title", backend.saved.single().subject)
    }

    @Test fun aSignatureStaysUnderTheSharedText() {
        val backend = FakeBackend(draft(body = "\n\n-- \nme"))
        IncomingShareIntake(backend, files).run(content(text = "hello"))
        val body = backend.saved.single().body
        assertTrue(body.startsWith("hello"))
        assertTrue(body.endsWith("-- \nme"))
    }

    @Test fun filesGoThroughTheAttachmentPathAndTheCopiesAreRemoved() {
        val backend = FakeBackend()
        val out = IncomingShareIntake(backend, files).run(content("content://a/one.png", "content://a/two.png"))
        assertEquals(ShareOutcome.Ready("d1", 2, 0), out)
        assertEquals(listOf("new", "attach:one.png", "attach:two.png"), backend.calls)
        assertTrue(made.none { it.file.exists() })
    }

    @Test fun aFileTheServerRefusesIsCountedAndTheRestGoOn() {
        val backend = FakeBackend().also { it.refuse = setOf("big.png"); it.breakAttach = setOf("net.png") }
        val out = IncomingShareIntake(backend, files).run(content("content://a/big.png", "content://a/net.png", "content://a/ok.png"))
        assertEquals(ShareOutcome.Ready("d1", 1, 2), out)
        assertTrue(made.none { it.file.exists() })
    }

    @Test fun anUnreadableFileAndSkippedOnesAreCountedAsLeftOut() {
        val backend = FakeBackend()
        val out = IncomingShareIntake(backend, files).run(content("content://a/unreadable", "content://a/ok.png", skipped = 2))
        assertEquals(ShareOutcome.Ready("d1", 1, 3), out)
    }

    @Test fun withoutADraftNothingIsLeftBehind() {
        val backend = FakeBackend().also { it.failNew = true }
        val out = IncomingShareIntake(backend, files).run(content("content://a/one.png", text = "x"))
        assertEquals(ShareOutcome.Failed, out)
        assertFalse(backend.calls.any { it.startsWith("attach") })
        assertTrue(made.none { it.file.exists() })
    }

    @Test fun aDraftThatCannotTakeTheTextIsThrownAway() {
        val backend = FakeBackend().also { it.failSave = true }
        val out = IncomingShareIntake(backend, files).run(content("content://a/one.png", text = "x"))
        assertEquals(ShareOutcome.Failed, out)
        assertEquals(listOf("new", "save", "discard"), backend.calls)
        assertTrue(made.none { it.file.exists() })
    }

    @Test fun aShareOfFilesOnlyDoesNotRewriteTheDraft() {
        val backend = FakeBackend()
        IncomingShareIntake(backend, files).run(content("content://a/one.png"))
        assertFalse("save" in backend.calls)
    }
}
