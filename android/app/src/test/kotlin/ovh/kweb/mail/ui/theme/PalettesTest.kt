package ovh.kweb.mail.ui.theme

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PalettesTest {
    @Test
    fun everyBuiltInThemeHasAllColoursInBothModes() {
        val builtIn = ThemeChoice.entries - ThemeChoice.Wallpaper
        assertEquals(builtIn.toSet(), PaletteTokens.palettes.keys)
        PaletteTokens.palettes.values.forEach { palette ->
            listOf(palette.light, palette.dark).forEach { line ->
                val colours = PaletteTokens.parse(line)
                assertEquals(PaletteTokens.ROLES.toSet(), colours.keys)
                assertTrue(colours.values.all { it ushr 24 == 0xFFL })
            }
        }
    }

    @Test
    fun aLineReadsAsOpaqueColoursByRole() {
        val ward = PaletteTokens.parse(PaletteTokens.palettes.getValue(ThemeChoice.Ward).light)
        assertEquals(0xFFB72046, ward.getValue("primary"))
        assertEquals(0xFFFFF8F7, ward.getValue("background"))
    }

    @Test
    fun theWallpaperIsTheDefaultOnlyWhereThePhoneHasIt() {
        assertEquals(ThemeChoice.Wallpaper, ThemeChoice.default(31))
        assertEquals(ThemeChoice.Ward, ThemeChoice.default(30))
        assertFalse(ThemeChoice.Wallpaper in ThemeChoice.offered(29))
        assertEquals(ThemeChoice.entries, ThemeChoice.offered(35))
    }

    @Test
    fun aStoredChoiceIsKeptAndAnUnusableOneFallsBack() {
        assertEquals(ThemeChoice.Moss, ThemeChoice.fromKey("moss", 29))
        assertEquals(ThemeChoice.Wallpaper, ThemeChoice.fromKey(null, 35))
        assertEquals(ThemeChoice.Ward, ThemeChoice.fromKey("wallpaper", 29))
        assertEquals(ThemeChoice.Wallpaper, ThemeChoice.fromKey("something old", 33))
    }
}
