package ovh.kweb.mail.ui.theme

/**
 * The themes the user can pick. [Wallpaper] takes the colours from the phone's wallpaper
 * (Material You, Android 12 and later); the others are built in and look the same on every phone.
 */
enum class ThemeChoice(val key: String) {
    Wallpaper("wallpaper"),
    Ward("ward"),
    Lavender("lavender"),
    Moss("moss"),
    Peach("peach"),
    ;

    companion object {
        /** What a fresh install uses: the wallpaper where the phone has such colours, else Ward. */
        fun default(sdk: Int): ThemeChoice = if (sdk >= WALLPAPER_FROM_SDK) Wallpaper else Ward

        /** The stored choice; an unknown key, or the wallpaper on a phone without it, is the default. */
        fun fromKey(key: String?, sdk: Int): ThemeChoice {
            val choice = entries.firstOrNull { it.key == key } ?: return default(sdk)
            return if (choice == Wallpaper && sdk < WALLPAPER_FROM_SDK) default(sdk) else choice
        }

        /** The choices to offer on this phone, in the order of the picker. */
        fun offered(sdk: Int): List<ThemeChoice> = entries.filter { it != Wallpaper || sdk >= WALLPAPER_FROM_SDK }

        /** Android 12. */
        const val WALLPAPER_FROM_SDK = 31
    }
}

/**
 * The colours of the built-in themes, as in the mock-ups (design/mockups-material.html), where
 * they can be tried side by side. One line per mode, twenty colours in the order of [ROLES].
 * No Android classes here, so the table is checked by a JVM test.
 */
object PaletteTokens {
    val ROLES = listOf(
        "background", "onSurface", "onSurfaceVariant", "primary", "onPrimary", "primaryContainer",
        "onPrimaryContainer", "secondaryContainer", "onSecondaryContainer", "tertiaryContainer",
        "onTertiaryContainer", "surfaceContainerLow", "surfaceContainer", "surfaceContainerHigh", "outline",
        "outlineVariant", "inverseSurface", "inverseOnSurface", "inversePrimary", "error",
    )

    class Palette(val seed: Long, val light: String, val dark: String)

    val palettes: Map<ThemeChoice, Palette> = mapOf(
        ThemeChoice.Ward to Palette(
            0xFFB72046,
            "fff8f7 22191a 534344 b72046 ffffff ffd9de 400012 fadde0 2c1519 ffddb9 2b1700 fff0f0 fceaea f6e3e4 857374 d8c2c3 382e2f ffecee ffb2bd ba1a1a",
            "1a1112 f0dedf d8c1c3 ffb2bd 670023 8f0b37 ffd9de 5c3f43 ffd9de 6b4300 ffddb9 231a1b 271e1f 322829 a08c8d 534344 f0dedf 382e2f b72046 ffb4ab",
        ),
        ThemeChoice.Lavender to Palette(
            0xFF6750A4,
            "fef7ff 1d1b20 49454f 6750a4 ffffff eaddff 21005d e8def8 1d192b ffd8e4 31111d f7f2fa f3edf7 ece6f0 7a757f cac4d0 322f35 f5eff7 d0bcff ba1a1a",
            "141218 e6e0e9 cac4d0 d0bcff 381e72 4f378b eaddff 4a4458 e8def8 633b48 ffd8e4 1d1b20 211f26 2b2930 938f99 49454f e6e0e9 322f35 6750a4 ffb4ab",
        ),
        ThemeChoice.Moss to Palette(
            0xFF3F6837,
            "f8fbf1 191d16 43483f 3f6837 ffffff c0efb1 002200 d7e8cd 131f0e bcebee 002022 f2f5eb ecefe5 e6e9e0 73796e c3c8bb 2e312b eff2e8 a5d396 ba1a1a",
            "11140f e1e4d9 c3c8bb a5d396 0a3909 275021 c0efb1 3b4b34 d7e8cd 1e4d51 bcebee 191d16 1d211a 272b24 8d9387 43483f e1e4d9 2e312b 3f6837 ffb4ab",
        ),
        ThemeChoice.Peach to Palette(
            0xFF8E4E1C,
            "fff8f5 221a15 52443c 8e4e1c ffffff ffdcc6 301400 f8dccd 2a1709 e2e4b3 1c1d00 fff1ea fbebe3 f5e5dc 85736a d7c2b8 382e29 ffede4 ffb68a ba1a1a",
            "19120d efded5 d7c2b8 ffb68a 4f2500 703700 ffdcc6 5a4235 f8dccd 494a1e e2e4b3 211a15 261e19 312823 9f8d83 52443c efded5 382e29 8e4e1c ffb4ab",
        ),
    )

    /** The colours of one line as opaque ARGB numbers, by role name. */
    fun parse(line: String): Map<String, Long> {
        val values = line.trim().split(' ')
        require(values.size == ROLES.size) { "a palette line needs ${ROLES.size} colours, got ${values.size}" }
        return ROLES.zip(values.map { 0xFF000000 or it.toLong(16) }).toMap()
    }
}
