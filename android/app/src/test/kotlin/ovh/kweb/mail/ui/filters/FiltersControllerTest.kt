package ovh.kweb.mail.ui.filters

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
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.FilterRule
import uniffi.kweb_core.Filters
import uniffi.kweb_core.Mailbox

private class FakeBackend : FiltersBackend {
    var current: Filters = ovh.kweb.mail.ui.filters.filters()
    var mailboxes: List<Mailbox> = listOf(box("work", "Work"), box("in", "Inbox", role = "inbox"))
    val calls = CopyOnWriteArrayList<String>()
    var failWith: Exception? = null
    var failSaveOnce: Exception? = null
    var gate: (() -> Unit)? = null
    var fromMessage: FilterRule = FilterRules.newRule()
    var saved: List<FilterRule> = emptyList()

    override fun filters(): Filters {
        val before = current
        gate?.invoke()
        calls += "filters"
        failWith?.let { throw it }
        return before
    }

    override fun saveFilters(rules: List<FilterRule>, activate: Boolean, overwrite: Boolean): Filters {
        calls += "save:${rules.size}:activate=$activate:overwrite=$overwrite"
        failSaveOnce?.let { failSaveOnce = null; throw it }
        failWith?.let { throw it }
        saved = rules
        current = current.copy(rules = rules.mapIndexed { i, r -> if (r.id.isEmpty()) r.copy(id = "new$i") else r })
        return current
    }

    override fun setFiltersActive(active: Boolean): Filters {
        calls += "active:$active"
        failWith?.let { throw it }
        current = current.copy(isActive = active, awayIsOn = false, otherActive = null)
        return current
    }

    override fun filterFromMessage(messageId: String): FilterRule {
        calls += "from:$messageId"
        return fromMessage
    }

    override fun mailboxes() = mailboxes
}

class FiltersControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeBackend
    private lateinit var c: FiltersController

    @Before fun setUp() {
        backend = FakeBackend()
        c = FiltersController(scope, pool.asCoroutineDispatcher(), backend)
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

    private fun ready() {
        c.start(null)
        waitFor("ready") { c.state.value.phase == FiltersPhase.READY }
    }

    private fun idle() = waitFor("idle") { !c.state.value.busy }

    @Test fun loadsRulesAndFolders() {
        backend.current = filters(listOf(rule("a"), rule("b")), active = true)
        ready()
        assertEquals(2, c.state.value.rules.size)
        assertEquals(2, c.state.value.folders.size)
        assertTrue(c.state.value.filters!!.isActive)
    }

    @Test fun aServerWithoutFiltersSaysSoInTheCoresWords() {
        backend.failWith = CoreException.Invalid("this server has no mail filters")
        c.start(null)
        waitFor("none") { c.state.value.phase == FiltersPhase.NONE }
        assertEquals(UiMessage.Text("this server has no mail filters"), c.state.value.unsupported)
    }

    @Test fun aFailedLoadCanBeRetried() {
        backend.failWith = java.io.IOException("x")
        c.start(null)
        waitFor("failed") { c.state.value.phase == FiltersPhase.FAILED }
        backend.failWith = null
        c.load()
        waitFor("ready") { c.state.value.phase == FiltersPhase.READY }
    }

    @Test fun movingARuleSavesAtOnceWithoutActivatingOrOverwriting() {
        backend.current = filters(listOf(rule("a"), rule("b")))
        ready()
        c.moveRule(0, 1)
        idle()
        assertEquals("save:2:activate=false:overwrite=false", backend.calls.last())
        assertEquals(listOf("b", "a"), c.state.value.rules.map { it.id })
    }

    @Test fun theRuleSwitchSavesAtOnce() {
        backend.current = filters(listOf(rule("a")))
        ready()
        c.setRuleEnabled(0, false)
        idle()
        assertEquals(false, c.state.value.rules[0].enabled)
    }

    @Test fun switchingOnWithTheAwayMessageOnAsksFirst() {
        backend.current = filters(listOf(rule("a")), away = true)
        ready()
        c.setActive(true)
        assertEquals(FiltersDialog.SwitchOn(listOf(SwitchOff.Away)), c.state.value.dialog)
        assertTrue(backend.calls.none { it.startsWith("active") })
        c.confirmSwitchOn()
        idle()
        assertEquals("active:true", backend.calls.last())
        assertNull(c.state.value.dialog)
        assertEquals(false, c.state.value.filters!!.awayIsOn)
    }

    @Test fun switchingOnWithNothingElseOnAsksNothingAndOffNeverAsks() {
        backend.current = filters(listOf(rule("a")))
        ready()
        c.setActive(true)
        idle()
        assertEquals("active:true", backend.calls.last())
        c.setActive(false)
        idle()
        assertEquals("active:false", backend.calls.last())
    }

    @Test fun savingANewRuleAddsItAtTheTopAndClosesTheEditor() {
        backend.current = filters(listOf(rule("a")))
        ready()
        c.addRule()
        c.edit { it.copy(name = "N", conditions = listOf(it.conditions[0].copy(value = "x")), markRead = true) }
        assertNull(c.problem())
        c.saveEditor()
        idle()
        assertEquals("N", backend.saved.first().name)
        assertEquals(2, backend.saved.size)
        assertNull(c.state.value.editor)
    }

    @Test fun anIncompleteRuleIsNotSent() {
        ready()
        c.addRule()
        assertEquals(RuleProblem.BlankValue(0), c.problem())
        c.saveEditor()
        assertTrue(backend.calls.none { it.startsWith("save") })
        assertNotNull(c.state.value.editor)
    }

    @Test fun theCoresRefusalIsShownAsItIsAndTheEditorStaysOpen() {
        backend.current = filters(listOf(rule("a", moveTo = "work")))
        ready()
        c.openRule(0)
        c.edit { it.copy(star = true) }
        backend.failWith = CoreException.Invalid("rule Boss: the folder is gone")
        c.saveEditor()
        idle()
        assertEquals(UiMessage.Text("rule Boss: the folder is gone"), c.state.value.error)
        assertNotNull(c.state.value.editor)
        assertEquals(false, c.state.value.rules[0].star)
    }

    @Test fun changedElsewhereReadsAgainAndSaysSo() {
        backend.current = filters(listOf(rule("a")))
        ready()
        backend.current = filters(listOf(rule("a"), rule("z")))
        backend.failSaveOnce = CoreException.Invalid("the filters changed elsewhere")
        c.setRuleEnabled(0, false)
        waitFor("reloaded") { c.state.value.info != null }
        assertEquals(2, c.state.value.rules.size)
        assertNull(c.state.value.error)
        assertTrue(c.state.value.info is UiMessage.Res)
    }

    @Test fun leavingTheEditorAsksOnlyWhenSomethingChanged() {
        backend.current = filters(listOf(rule("a")))
        ready()
        c.openRule(0)
        c.leaveEditor()
        assertNull(c.state.value.editor)
        c.openRule(0)
        c.edit { it.copy(name = "other") }
        c.leaveEditor()
        assertEquals(FiltersDialog.DiscardEditor, c.state.value.dialog)
        assertNotNull(c.state.value.editor)
        c.discardEditor()
        assertNull(c.state.value.editor)
        assertNull(c.state.value.dialog)
    }

    @Test fun deletingARuleAsksThenSavesTheListWithoutIt() {
        backend.current = filters(listOf(rule("a"), rule("b")))
        ready()
        c.openRule(1)
        c.askDeleteRule()
        assertEquals(FiltersDialog.DeleteRule, c.state.value.dialog)
        c.confirmDeleteRule()
        idle()
        assertEquals(listOf("a"), backend.saved.map { it.id })
        assertNull(c.state.value.editor)
    }

    @Test fun startingOverSavesAnEmptyListWithOverwrite() {
        backend.current = filters(edited = true)
        ready()
        c.askStartOver()
        assertEquals(FiltersDialog.StartOver, c.state.value.dialog)
        c.confirmStartOver()
        idle()
        assertEquals("save:0:activate=false:overwrite=true", backend.calls.last())
    }

    @Test fun updateSavesTheRulesAgain() {
        backend.current = filters(listOf(rule("a")), stale = true)
        ready()
        c.update()
        idle()
        assertEquals("save:1:activate=false:overwrite=false", backend.calls.last())
    }

    @Test fun aRuleFromAMessageOpensTheEditorAsNew() {
        backend.fromMessage = rule("", name = "", conditions = listOf(uniffi.kweb_core.FilterCondition(uniffi.kweb_core.FilterField.From, uniffi.kweb_core.FilterMatch.CONTAINS, "a@b.c")))
        c.start("m1")
        waitFor("editor") { c.state.value.editor != null }
        assertEquals(true, c.state.value.editor!!.isNew)
        assertEquals("a@b.c", c.state.value.editor!!.rule.conditions[0].value)
        assertEquals(false, c.state.value.editor!!.dirty)
    }

    @Test fun aLateAnswerDoesNotReplaceANewerOne() {
        backend.current = filters(listOf(rule("old")))
        val release = CountDownLatch(1)
        val entered = CountDownLatch(1)
        backend.gate = {
            entered.countDown()
            release.await(5, TimeUnit.SECONDS)
        }
        c.start(null)
        assertTrue(entered.await(5, TimeUnit.SECONDS))
        // A newer read starts and finishes first; the older one is released afterwards.
        backend.gate = null
        backend.current = filters(listOf(rule("new")))
        c.load()
        waitFor("new answer") { c.state.value.rules.map { it.id } == listOf("new") }
        release.countDown()
        Thread.sleep(200)
        assertEquals(listOf("new"), c.state.value.rules.map { it.id })
    }

    @Test fun aChangeIsRefusedWhileAnotherIsBeingSaved() {
        backend.current = filters(listOf(rule("a"), rule("b")))
        ready()
        val release = CountDownLatch(1)
        val entered = CountDownLatch(1)
        backend.gate = null
        val slow = object : FiltersBackend by backend {
            override fun saveFilters(rules: List<FilterRule>, activate: Boolean, overwrite: Boolean): Filters {
                entered.countDown()
                release.await(5, TimeUnit.SECONDS)
                return backend.saveFilters(rules, activate, overwrite)
            }
        }
        val c2 = FiltersController(scope, pool.asCoroutineDispatcher(), slow)
        c2.start(null)
        waitFor("ready2") { c2.state.value.phase == FiltersPhase.READY }
        c2.moveRule(0, 1)
        assertTrue(entered.await(5, TimeUnit.SECONDS))
        c2.moveRule(1, -1)
        release.countDown()
        waitFor("done") { !c2.state.value.busy }
        assertEquals(1, backend.calls.count { it.startsWith("save") })
    }

    @Test fun switchingOnWithNoScriptYetSavesTheRulesActive() {
        backend.current = filters(listOf(rule("a")), text = "")
        ready()
        c.setActive(true)
        idle()
        assertEquals("save:1:activate=true:overwrite=false", backend.calls.last())
        assertTrue(backend.calls.none { it.startsWith("active") })
    }

    @Test fun switchingOnWithNoScriptAndAwayOnAsksThenSavesActive() {
        backend.current = filters(away = true, text = "")
        ready()
        c.setActive(true)
        assertNotNull(c.state.value.dialog)
        c.confirmSwitchOn()
        idle()
        assertEquals("save:0:activate=true:overwrite=false", backend.calls.last())
    }

    @Test fun anEditedScriptOpensNoEditor() {
        backend.current = filters(edited = true)
        backend.fromMessage = rule("", name = "")
        c.start("m1")
        waitFor("info") { c.state.value.info != null }
        assertNull(c.state.value.editor)
        c.addRule()
        assertNull(c.state.value.editor)
    }

    @Test fun aNewRuleAlreadySavedIsNotSavedTwiceAfterChangedElsewhere() {
        backend.current = filters(listOf(rule("a")))
        ready()
        c.addRule()
        c.edit { it.copy(name = "N", conditions = listOf(it.conditions[0].copy(value = "x")), markRead = true) }
        val typed = c.state.value.editor!!.rule
        // The first save was written but its answer was lost; the next one is refused.
        backend.current = filters(listOf(typed.copy(id = "z"), rule("a")))
        backend.failSaveOnce = CoreException.Invalid("the filters changed elsewhere")
        c.saveEditor()
        waitFor("closed") { c.state.value.editor == null && c.state.value.info != null }
        assertEquals(2, c.state.value.rules.size)
        assertEquals(1, backend.calls.count { it.startsWith("save") })
    }

    @Test fun nothingTypedWhileASaveRunsChangesTheEditor() {
        backend.current = filters(listOf(rule("a")))
        ready()
        c.openRule(0)
        c.edit { it.copy(star = true) }
        val release = CountDownLatch(1)
        val entered = CountDownLatch(1)
        val slow = object : FiltersBackend by backend {
            override fun saveFilters(rules: List<FilterRule>, activate: Boolean, overwrite: Boolean): Filters {
                entered.countDown()
                release.await(5, TimeUnit.SECONDS)
                return backend.saveFilters(rules, activate, overwrite)
            }
        }
        val c2 = FiltersController(scope, pool.asCoroutineDispatcher(), slow)
        c2.start(null)
        waitFor("r2") { c2.state.value.phase == FiltersPhase.READY }
        c2.openRule(0)
        c2.edit { it.copy(star = true) }
        c2.saveEditor()
        assertTrue(entered.await(5, TimeUnit.SECONDS))
        c2.edit { it.copy(name = "late") }
        assertEquals("Boss", c2.state.value.editor!!.rule.name)
        release.countDown()
    }

    @Test fun aRefusedQuietReadKeepsTheListAndShowsAnError() {
        backend.current = filters(listOf(rule("a"), rule("b")))
        ready()
        backend.failWith = CoreException.Invalid("the server does not have this file")
        c.refresh()
        waitFor("error") { c.state.value.error != null }
        assertEquals(FiltersPhase.READY, c.state.value.phase)
        assertEquals(2, c.state.value.rules.size)
    }
}
