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
    fun crimsonIsTheDefaultAndTheWallpaperIsOfferedOnlyWhereThePhoneHasIt() {
        assertEquals(ThemeChoice.Ward, ThemeChoice.default(31))
        assertEquals(ThemeChoice.Wallpaper, ThemeChoice.fromKey("wallpaper", 31))
        assertEquals(ThemeChoice.Ward, ThemeChoice.default(30))
        assertFalse(ThemeChoice.Wallpaper in ThemeChoice.offered(29))
        assertEquals(ThemeChoice.entries, ThemeChoice.offered(35))
    }

    @Test
    fun aStoredChoiceIsKeptAndAnUnusableOneFallsBack() {
        assertEquals(ThemeChoice.Moss, ThemeChoice.fromKey("moss", 29))
        assertEquals(ThemeChoice.Ward, ThemeChoice.fromKey(null, 35))
        assertEquals(ThemeChoice.Ward, ThemeChoice.fromKey("wallpaper", 29))
        assertEquals(ThemeChoice.Ward, ThemeChoice.fromKey("something old", 33))
    }

    @Test
    fun theSixNewThemesAreThereWithStableKeysAfterPeach() {
        val keys = listOf("ocean", "slate", "sand", "rose", "sage", "graphite")
        assertEquals(keys, ThemeChoice.entries.drop(ThemeChoice.entries.indexOf(ThemeChoice.Peach) + 1).map { it.key })
        keys.forEach { assertEquals(it, ThemeChoice.fromKey(it, 35).key) }
        assertEquals(ThemeChoice.Moss, ThemeChoice.fromKey("moss", 35))
    }

    private fun luminance(argb: Long): Double {
        fun channel(shift: Int): Double {
            val v = ((argb shr shift) and 0xFF) / 255.0
            return if (v <= 0.03928) v / 12.92 else Math.pow((v + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    private fun contrast(a: Long, b: Long): Double {
        val (hi, lo) = luminance(a).let { x -> luminance(b).let { y -> maxOf(x, y) to minOf(x, y) } }
        return (hi + 0.05) / (lo + 0.05)
    }

    @Test
    fun everyOnColourReadsOnItsBackgroundInEveryTheme() {
        val pairs = listOf(
            "onSurface" to "background", "onSurfaceVariant" to "background", "onPrimary" to "primary",
            "onPrimaryContainer" to "primaryContainer", "onSecondaryContainer" to "secondaryContainer",
            "onTertiaryContainer" to "tertiaryContainer", "inverseOnSurface" to "inverseSurface",
            "onSurface" to "surfaceContainer", "onSurface" to "surfaceContainerHigh",
            "onSurfaceVariant" to "surfaceContainer",
        )
        PaletteTokens.palettes.forEach { (theme, palette) ->
            listOf(palette.light, palette.dark).forEach { line ->
                val c = PaletteTokens.parse(line)
                pairs.forEach { (fg, bg) ->
                    val ratio = contrast(c.getValue(fg), c.getValue(bg))
                    assertTrue("$theme $fg on $bg is only $ratio", ratio >= 4.5)
                }
            }
        }
    }
}
