package ovh.kweb.mail.ui.sending

import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.IdentityDetail
import uniffi.kweb_core.Exception as CoreException

private fun sender(id: String, email: String = "$id@x.org", name: String = "N$id", default: Boolean = false, mayDelete: Boolean = true) =
    IdentityDetail(id, name, email, "", "", default, mayDelete)

private class FakeSenders : SendersBackend {
    var items = mutableListOf(sender("a", default = true), sender("b"))
    val calls = CopyOnWriteArrayList<String>()
    var failRead: Exception? = null
    var failSave: Exception? = null
    var failDefault: Exception? = null
    var failDelete: Exception? = null
    var nextId = 1

    override fun identityDetails(): List<IdentityDetail> {
        calls += "list"
        failRead?.let { throw it }
        return items.toList()
    }

    override fun saveIdentity(id: String?, name: String, email: String, replyTo: String, signature: String): IdentityDetail {
        calls += "save:${id ?: "new"}:$name:$email:$replyTo:$signature"
        failSave?.let { throw it }
        val made = if (id == null) sender("n${nextId++}", email, name) else items.first { it.id == id }.copy(name = name)
        val stored = made.copy(replyTo = replyTo, signature = signature)
        val at = items.indexOfFirst { it.id == stored.id }
        if (at >= 0) items[at] = stored else items += stored
        return stored
    }

    override fun deleteIdentity(id: String) {
        calls += "delete:$id"
        failDelete?.let { throw it }
        items.removeAll { it.id == id }
    }

    override fun setDefaultIdentity(id: String) {
        calls += "default:$id"
        failDefault?.let { throw it }
        items.replaceAll { it.copy(isDefault = it.id == id) }
    }
}

class SendersRulesTest {
    @Test fun addressesAreChecked() {
        assertTrue(SendersRules.looksLikeAddress(" me@example.org "))
        assertFalse(SendersRules.looksLikeAddress("me"))
        assertFalse(SendersRules.looksLikeAddress("@example.org"))
        assertFalse(SendersRules.looksLikeAddress("me@"))
        assertFalse(SendersRules.looksLikeAddress("a@b@c"))
        assertFalse(SendersRules.looksLikeAddress("me @example.org"))
    }

    @Test fun theLimitsAreCountedInCharacters() {
        val emoji = "😀"
        assertEquals(1, SendersRules.length(emoji))
        assertNull(SendersRules.problem(SendersRules.blank().copy(email = "a@b.c", signature = "x".repeat(SendersRules.SIGNATURE_LIMIT))))
        assertEquals(SendersRules.Problem.SignatureTooLong, SendersRules.problem(SendersRules.blank().copy(email = "a@b.c", signature = "x".repeat(SendersRules.SIGNATURE_LIMIT + 1))))
        assertEquals(SendersRules.Problem.NameTooLong, SendersRules.problem(SendersRules.blank().copy(email = "a@b.c", name = "x".repeat(SendersRules.NAME_LIMIT + 1))))
    }

    @Test fun aNewSenderNeedsAnAddressAnExistingOneDoesNot() {
        assertEquals(SendersRules.Problem.NoEmail, SendersRules.problem(SendersRules.blank()))
        assertEquals(SendersRules.Problem.BadEmail, SendersRules.problem(SendersRules.blank().copy(email = "nope")))
        assertNull(SendersRules.problem(SendersRules.form(sender("a")).copy(email = "")))
    }

    @Test fun aReplyToIsFreeButMustLookLikeAnAddress() {
        val form = SendersRules.form(sender("a"))
        assertNull(SendersRules.problem(form.copy(replyTo = "")))
        assertNull(SendersRules.problem(form.copy(replyTo = "x@elsewhere.example")))
        assertEquals(SendersRules.Problem.BadReplyTo, SendersRules.problem(form.copy(replyTo = "elsewhere")))
    }

    @Test fun theDefaultIsOnlyAskedForWhenItChanges() {
        val form = SendersRules.form(sender("a", default = true))
        assertFalse(SendersRules.makesDefault(form))
        assertTrue(SendersRules.makesDefault(SendersRules.form(sender("b")).copy(useForNew = true)))
        assertFalse(SendersRules.makesDefault(SendersRules.form(sender("b"))))
    }

    @Test fun aRowShowsTheNameAndTheAddress() {
        assertEquals("Ann", SendersRules.title(sender("a", name = "Ann")))
        assertEquals("a@x.org", SendersRules.subtitle(sender("a", name = "Ann")))
        assertEquals("a@x.org", SendersRules.title(sender("a", name = "")))
        assertNull(SendersRules.subtitle(sender("a", name = "")))
    }

    @Test fun signaturesLoseCarriageReturnsAndTrailingSpace() {
        assertEquals("a\nb", SendersRules.cleanSignature("a\r\nb\n\n "))
    }
}

class SendersControllerTest {
    private fun controller(b: FakeSenders) = SendersController(CoroutineScope(SupervisorJob() + Dispatchers.Unconfined), Dispatchers.Unconfined, b).also { it.start() }

    @Test fun startReadsTheSenders() {
        val c = controller(FakeSenders())
        assertEquals(SendersPhase.READY, c.state.phase)
        assertEquals(listOf("a", "b"), c.state.items.map { it.id })
    }

    @Test fun aFailedFirstReadIsAFailure() {
        val b = FakeSenders().also { it.failRead = CoreException.Temporary("offline") }
        val c = controller(b)
        assertEquals(SendersPhase.FAILED, c.state.phase)
        b.failRead = null
        c.load()
        assertEquals(SendersPhase.READY, c.state.phase)
    }

    @Test fun savingChangesTheNameAndTheSignatureAndClosesTheForm() {
        val b = FakeSenders()
        val c = controller(b)
        c.open("b")
        c.setName("  New name ")
        c.setSignature("Bye\r\nMe")
        c.setReplyTo(" r@x.org ")
        c.save()
        assertTrue(b.calls.contains("save:b:New name:b@x.org:r@x.org:Bye\nMe"))
        assertNull(c.state.editor)
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.senders_saved), c.state.info)
        assertEquals("New name", c.state.items.first { it.id == "b" }.name)
    }

    @Test fun theDefaultIsSetAfterTheSaveWhenSwitchedOn() {
        val b = FakeSenders()
        val c = controller(b)
        c.open("b")
        c.setUseForNew(true)
        c.save()
        assertEquals(listOf("save:b:Nb:b@x.org::", "default:b"), b.calls.filter { it != "list" })
        assertTrue(c.state.items.first { it.id == "b" }.isDefault)
        assertFalse(c.state.items.first { it.id == "a" }.isDefault)
    }

    @Test fun theDefaultSenderCannotBeSwitchedOff() {
        val c = controller(FakeSenders())
        c.open("a")
        c.setUseForNew(false)
        assertTrue(c.state.editor!!.useForNew)
    }

    @Test fun aSavedSenderWhoseDefaultFailedIsStoredAndSaysSo() {
        val b = FakeSenders().also { it.failDefault = CoreException.Temporary("offline") }
        val c = controller(b)
        c.open("b")
        c.setUseForNew(true)
        c.save()
        assertNull(c.state.editor)
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.senders_default_failed), c.state.error)
        assertFalse(c.state.busy)
    }

    @Test fun theCoresRefusalIsShownAndTheFormStaysOpen() {
        val b = FakeSenders().also { it.failSave = CoreException.Invalid("This address does not belong to the account.") }
        val c = controller(b)
        c.openNew()
        c.setEmail("me@elsewhere.example")
        c.save()
        assertEquals(UiMessage.Text("This address does not belong to the account."), c.state.error)
        assertNotNull(c.state.editor)
        assertFalse(c.state.busy)
    }

    @Test fun aFormWithAProblemIsNotSent() {
        val b = FakeSenders()
        val c = controller(b)
        c.openNew()
        c.setEmail("nope")
        c.save()
        assertEquals(SendersRules.Problem.BadEmail, c.state.problem)
        assertTrue(b.calls.none { it.startsWith("save") })
        c.setEmail("me@x.org")
        assertNull(c.state.problem)
    }

    @Test fun anAddressIsFixedOnceTheSenderExists() {
        val c = controller(FakeSenders())
        c.open("b")
        c.setEmail("other@x.org")
        assertEquals("b@x.org", c.state.editor!!.email)
    }

    @Test fun leavingAChangedFormAsksFirst() {
        val c = controller(FakeSenders())
        c.open("b")
        c.leaveEditor()
        assertNull(c.state.editor)
        c.open("b")
        c.setName("changed")
        c.leaveEditor()
        assertEquals(SendersDialog.Discard, c.state.dialog)
        assertNotNull(c.state.editor)
        c.discardEditor()
        assertNull(c.state.editor)
        assertNull(c.state.dialog)
    }

    @Test fun deletingAsksFirstAndOnlyWhenAllowed() {
        val b = FakeSenders().also { it.items = mutableListOf(sender("a", default = true, mayDelete = false), sender("b")) }
        val c = controller(b)
        c.open("a")
        c.askDelete()
        assertNull(c.state.dialog)
        c.leaveEditor()
        c.open("b")
        c.askDelete()
        assertEquals(SendersDialog.Delete, c.state.dialog)
        assertTrue(b.calls.none { it.startsWith("delete") })
        c.confirmDelete()
        assertTrue(b.calls.contains("delete:b"))
        assertEquals(listOf("a"), c.state.items.map { it.id })
        assertNull(c.state.editor)
    }

    @Test fun aDeleteWhoseReReadFailsStillTakesTheSenderOffTheList() {
        val b = FakeSenders()
        val c = controller(b)
        c.open("b")
        c.askDelete()
        b.failRead = CoreException.Temporary("offline")
        c.confirmDelete()
        assertEquals(listOf("a"), c.state.items.map { it.id })
        assertNull(c.state.error)
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.senders_deleted), c.state.info)
    }

    @Test fun aRefusedDeleteKeepsTheFormAndShowsTheCoresWords() {
        val b = FakeSenders().also { it.failDelete = CoreException.Invalid("A waiting message uses this sender.") }
        val c = controller(b)
        c.open("b")
        c.askDelete()
        c.confirmDelete()
        assertEquals(UiMessage.Text("A waiting message uses this sender."), c.state.error)
        assertNotNull(c.state.editor)
        assertFalse(c.state.busy)
    }

    @Test fun resetForgetsEverything() {
        val c = controller(FakeSenders())
        c.open("b")
        c.reset()
        assertEquals(SendersState(), c.state)
    }
}
