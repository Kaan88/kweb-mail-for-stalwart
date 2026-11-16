package ovh.kweb.mail.ui.theme

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ThemeRulesTest {
    @Test
    fun modeFollowsThePhoneOnlyInSystem() {
        assertTrue(ThemeMode.System.isDark(true))
        assertFalse(ThemeMode.System.isDark(false))
        assertTrue(ThemeMode.Dark.isDark(false))
        assertFalse(ThemeMode.Light.isDark(true))
    }

    @Test
    fun unknownModeKeyIsSystem() {
        assertEquals(ThemeMode.System, ThemeMode.of(null))
        assertEquals(ThemeMode.System, ThemeMode.of("sepia"))
        ThemeMode.entries.forEach { assertEquals(it, ThemeMode.of(it.key)) }
    }

    @Test
    fun blackSwitchIsOffLimitsInLightOnly() {
        assertFalse(BlackRules.switchEnabled(ThemeMode.Light))
        assertTrue(BlackRules.switchEnabled(ThemeMode.System))
        assertTrue(BlackRules.switchEnabled(ThemeMode.Dark))
    }

    @Test
    fun blackIsDrawnOnlyWhenDarkAndAsked() {
        assertTrue(BlackRules.active(true, ThemeMode.Dark, systemDark = false))
        assertTrue(BlackRules.active(true, ThemeMode.System, systemDark = true))
        assertFalse(BlackRules.active(true, ThemeMode.System, systemDark = false))
        assertFalse(BlackRules.active(true, ThemeMode.Light, systemDark = true))
        assertFalse(BlackRules.active(false, ThemeMode.Dark, systemDark = true))
    }

    @Test
    fun layersStepUpFromBlack() {
        val steps = listOf(BlackRules.SURFACE, BlackRules.CONTAINER_LOW, BlackRules.CONTAINER, BlackRules.CONTAINER_HIGH, BlackRules.CONTAINER_HIGHEST)
        assertEquals(0xFF000000L, steps.first())
        assertEquals(steps.sorted(), steps)
        assertEquals(steps.size, steps.toSet().size)
    }
}
