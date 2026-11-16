package ovh.kweb.mail.ui.filters

import java.time.ZoneId
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
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
import uniffi.kweb_core.FiltersState
import uniffi.kweb_core.Label
import uniffi.kweb_core.Mailbox

private const val A = """{"id":"a","name":"A","enabled":true,"matchType":"all","conditions":[{"field":"from","comparator":"contains","value":"a@x.org"}],"actions":[{"type":"move","value":"Work","mailboxId":"work"}],"stopProcessing":false}"""
private const val B = """{"id":"b","name":"B","enabled":true,"matchType":"all","conditions":[{"field":"subject","comparator":"is","value":"hi"}],"actions":[{"type":"star"}],"stopProcessing":false}"""

private val utc = ZoneId.of("UTC")

private class FakeBackend : FiltersBackend {
    var current: FiltersState = coreState("[$A,$B]")
    var mailboxes: List<Mailbox> = listOf(box("work", "Work"), box("in", "Inbox", role = "inbox"))
    val calls = CopyOnWriteArrayList<String>()
    var failWith: Exception? = null
    var failOnce: Exception? = null
    var seed: FilterRule? = null

    private fun write(call: String, next: () -> FiltersState): FiltersState {
        calls += call
        failOnce?.let { failOnce = null; throw it }
        failWith?.let { throw it }
        current = next()
        return current
    }

    private fun rules() = RuleModel.parseList(current.rulesJson, utc)

    override fun state(): FiltersState {
        calls += "state"
        failWith?.let { throw it }
        return current
    }

    override fun save(rulesJson: String) = write("save") { current.copy(rulesJson = rulesJson, active = true, scriptName = "filters", migrationPending = false) }
    override fun setEnabled(on: Boolean) = write("enabled:$on") { current.copy(active = on, vacationConflict = false) }
    override fun toggle(id: String, enabled: Boolean) = write("toggle:$id:$enabled") {
        current.copy(rulesJson = RuleModel.listToJson(rules().map { if (it.id == id) it.copy(enabled = enabled) else it }, utc))
    }
    override fun move(id: String, up: Boolean) = write("move:$id:$up") {
        val list = rules().toMutableList()
        val i = list.indexOfFirst { it.id == id }
        val j = if (up) i - 1 else i + 1
        if (i >= 0 && j in list.indices) {
            val x = list[i]
            list[i] = list[j]
            list[j] = x
        }
        current.copy(rulesJson = RuleModel.listToJson(list, utc))
    }
    override fun delete(id: String) = write("delete:$id") { current.copy(rulesJson = RuleModel.listToJson(rules().filter { it.id != id }, utc)) }
    override fun rawSave(text: String) = write("raw") { current.copy(rawText = text) }
    override fun resetToVisual() = write("reset") { current.copy(rulesJson = "[]", opaque = false, foreign = false, rawText = "") }
    override fun migrateOld() = write("migrate") { current.copy(oldScriptPresent = false) }
    override fun mailboxes() = mailboxes
    override fun labels() = listOf(Label("Imp", "Imp"))
    override fun seed(messageId: String): FilterRule {
        calls += "seed:$messageId"
        return seed!!
    }
}

class FiltersControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeBackend
    private lateinit var c: FiltersController

    @Before fun setUp() {
        backend = FakeBackend()
        c = FiltersController(scope, pool.asCoroutineDispatcher(), backend, utc)
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

    private fun ready(from: String? = null) {
        c.start(from)
        waitFor("ready") { c.state.value.phase == FiltersPhase.READY }
    }

    private fun idle() = waitFor("idle") { !c.state.value.busy }

    @Test fun loadsRulesFoldersAndLabels() {
        ready()
        assertEquals(listOf("a", "b"), c.state.value.rules.map { it.id })
        assertEquals(2, c.state.value.folders.size)
        assertEquals(listOf("Imp"), c.state.value.labels.map { it.name })
    }

    @Test fun aServerWithoutFiltersSaysSo() {
        backend.current = coreState(supported = false)
        c.start(null)
        waitFor("none") { c.state.value.phase == FiltersPhase.NONE }
        assertNotNull(c.state.value.unsupported)
    }

    @Test fun theCoresWordsAreShownWhenTheServerRefuses() {
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

    @Test fun movingARuleAndTheRuleSwitchSaveAtOnce() {
        ready()
        c.moveRule("a", up = false)
        idle()
        assertEquals(listOf("b", "a"), c.state.value.rules.map { it.id })
        c.setRuleEnabled("b", false)
        idle()
        assertEquals(false, c.state.value.rules.first { it.id == "b" }.enabled)
        assertEquals(listOf("state", "move:a:false", "toggle:b:false"), backend.calls.toList())
    }

    @Test fun deletingOffersUndoWhichSavesTheListAsItWas() {
        ready()
        val before = c.state.value.core!!.rulesJson
        c.deleteRule("a")
        idle()
        assertEquals(listOf("b"), c.state.value.rules.map { it.id })
        assertEquals(UndoDelete("A", before), c.state.value.undo)
        c.undoDelete()
        idle()
        assertEquals(listOf("a", "b"), c.state.value.rules.map { it.id })
        assertNull(c.state.value.undo)
        assertEquals("save", backend.calls.last())
    }

    @Test fun switchingOnWithTheAwayMessageInTheWayAsksFirst() {
        backend.current = coreState("[$A]", active = false, vacationConflict = true)
        ready()
        c.setEnabled(true)
        assertEquals(FiltersDialog.SwitchOn, c.state.value.dialog)
        assertTrue(backend.calls.none { it.startsWith("enabled") })
        c.confirmSwitchOn()
        idle()
        assertEquals("enabled:true", backend.calls.last())
        assertTrue(c.state.value.core!!.active)
    }

    @Test fun switchingOnOrOffAsksNothingElse() {
        backend.current = coreState("[$A]", active = false)
        ready()
        c.setEnabled(true)
        idle()
        assertEquals("enabled:true", backend.calls.last())
        c.setEnabled(false)
        idle()
        assertEquals("enabled:false", backend.calls.last())
    }

    @Test fun withNoScriptYetSwitchingOnSavesTheRulesWhichMakesIt() {
        backend.current = coreState("[]", active = false, scriptName = "")
        ready()
        c.setEnabled(true)
        idle()
        assertEquals("save", backend.calls.last())
        assertTrue(c.state.value.core!!.active)
    }

    @Test fun savingANewRuleAddsItAndClosesTheEditor() {
        ready()
        c.addRule()
        c.edit { it.copy(name = "N", conditions = listOf(it.conditions[0].copy(value = "x")), actions = listOf(ActDraft("star"))) }
        assertNull(c.problem())
        c.saveEditor()
        idle()
        assertEquals(listOf("a", "b"), c.state.value.rules.map { it.id }.take(2))
        assertEquals("N", c.state.value.rules.last().name)
        assertNull(c.state.value.editor)
    }

    @Test fun anIncompleteRuleIsNotSent() {
        ready()
        c.addRule()
        assertEquals(RuleProblem.EmptyName, c.problem())
        c.saveEditor()
        assertTrue(backend.calls.none { it == "save" })
        assertNotNull(c.state.value.editor)
    }

    @Test fun theCoresRefusalIsShownAndTheEditorStaysOpen() {
        ready()
        c.openRule("a")
        c.edit { it.copy(stop = true) }
        backend.failWith = CoreException.Invalid("The rule A has no folder to put mail in.")
        c.saveEditor()
        idle()
        assertEquals(UiMessage.Text("The rule A has no folder to put mail in."), c.state.value.error)
        assertNotNull(c.state.value.editor)
        assertEquals(false, c.state.value.rules.first { it.id == "a" }.stop)
    }

    @Test fun aRuleThatNamesAFolderByPathOnlyGetsItsId() {
        backend.current = coreState("[${A.replace(""","mailboxId":"work"""", "")}]")
        ready()
        c.openRule("a")
        assertEquals("work", c.state.value.editor!!.rule.actions[0].mailboxId)
        assertNull(c.problem())
    }

    @Test fun changedElsewhereReadsAgainAndSaysSo() {
        ready()
        backend.current = coreState("[$A,$B]")
        backend.failOnce = CoreException.Invalid("The filters changed elsewhere. Read them again.")
        c.setRuleEnabled("a", false)
        waitFor("info") { c.state.value.info != null && !c.state.value.busy }
        assertEquals("state", backend.calls.last())
    }

    @Test fun leavingTheEditorAsksOnlyWhenSomethingChanged() {
        ready()
        c.openRule("a")
        c.leaveEditor()
        assertNull(c.state.value.editor)
        c.openRule("a")
        c.edit { it.copy(name = "Z") }
        c.leaveEditor()
        assertEquals(FiltersDialog.DiscardEditor, c.state.value.dialog)
        c.discardEditor()
        assertNull(c.state.value.editor)
    }

    @Test fun anOpaqueScriptOffersTextOnly() {
        backend.current = coreState("[]", opaque = true, rawText = "# odd\nkeep;")
        ready()
        assertTrue(c.state.value.textOnly)
        c.addRule()
        assertNull(c.state.value.editor)
        c.openRaw()
        assertEquals("# odd\nkeep;", c.state.value.raw!!.text)
        c.editRaw("keep;")
        c.saveRaw()
        idle()
        assertEquals("raw", backend.calls.last())
        assertNull(c.state.value.raw)
    }

    @Test fun resetToVisualAsksFirst() {
        backend.current = coreState("[]", foreign = true, rawText = "keep;")
        ready()
        c.askReset()
        assertEquals(FiltersDialog.ResetToVisual, c.state.value.dialog)
        assertTrue(backend.calls.none { it == "reset" })
        c.confirmReset()
        idle()
        assertEquals("reset", backend.calls.last())
        assertTrue(!c.state.value.textOnly)
    }

    @Test fun theRawEditorOfARuleScriptStartsFromTheScriptText() {
        backend.current = coreState("[$A]", rawText = "# the script")
        ready()
        c.openRaw()
        c.leaveRaw()
        assertNull(c.state.value.raw)
        c.openRaw()
        c.editRaw("# changed")
        c.leaveRaw()
        assertEquals(FiltersDialog.DiscardRaw, c.state.value.dialog)
    }

    @Test fun migrationSavesTheConvertedRulesThenRemovesTheOldScript() {
        backend.current = coreState("[$A]", active = false, scriptName = "", migrationPending = true, oldScriptPresent = true)
        ready()
        c.addRule()
        assertNull(c.state.value.editor)
        c.askMigrate()
        assertEquals(FiltersDialog.Migrate(1), c.state.value.dialog)
        c.confirmMigrate()
        idle()
        assertEquals(listOf("save", "migrate"), backend.calls.takeLast(2))
        assertTrue(!c.state.value.core!!.migrationPending)
    }

    @Test fun filterMailLikeThisOpensTheEditorWithTheSender() {
        backend.seed = FilterRule(
            "", "Ann", true, true,
            listOf(uniffi.kweb_core.FilterCondition(uniffi.kweb_core.FilterField.From, uniffi.kweb_core.FilterMatch.CONTAINS, "ann@x.org")),
            null, false, false, false, false, false,
        )
        c.start("m1")
        waitFor("editor") { c.state.value.editor != null }
        assertTrue(c.state.value.editor!!.isNew)
        assertEquals("ann@x.org", c.state.value.editor!!.rule.conditions[0].value)
        assertTrue(backend.calls.none { it == "save" })
    }

    @Test fun filterMailLikeThisIsRefusedWithWordsWhenTheScriptIsText() {
        backend.current = coreState("[]", foreign = true, rawText = "keep;")
        c.start("m1")
        waitFor("info") { c.state.value.info != null }
        assertNull(c.state.value.editor)
        assertTrue(backend.calls.none { it.startsWith("seed") })
    }
}
