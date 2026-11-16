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
    Ocean("ocean"),
    Slate("slate"),
    Sand("sand"),
    Rose("rose"),
    Sage("sage"),
    Graphite("graphite"),
    ;

    companion object {
        /** What a fresh install uses: Ward crimson on every phone (the wallpaper palette is one tap away in the picker). */
        @Suppress("UNUSED_PARAMETER")
        fun default(sdk: Int): ThemeChoice = Ward

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
        ThemeChoice.Ocean to Palette(
            0xFF3D5F90,
            "f9f9ff 191c20 43474e 3d5f90 ffffff d5e3ff 001c3b d9e3f8 121c2b f8d8fe 27132f f3f3fa ededf4 e7e8ee 74777f c4c6cf 2e3035 f0f0f7 a7c8ff ba1a1a",
            "111318 e1e2e9 c4c6cf a7c8ff 03305f 234777 d5e3ff 3d4758 d9e3f8 553f5d f8d8fe 191c20 1d2024 282a2f 8d9199 43474e e1e2e9 2e3035 3d5f90 ffb4ab",
        ),
        ThemeChoice.Slate to Palette(
            0xFF2E628C,
            "f7f9ff 181c20 42474e 2e628c ffffff cde5ff 001d32 d5e4f6 0e1d2a eedcff 221533 f1f4f9 eceef3 e6e8ee 72777f c2c7cf 2d3135 eef1f6 9acbfa ba1a1a",
            "101418 e0e2e8 c2c7cf 9acbfa 003352 0c4a72 cde5ff 3a4857 d5e4f6 4f4061 eedcff 181c20 1c2024 272a2f 8c9198 42474e e0e2e8 2d3135 2e628c ffb4ab",
        ),
        ThemeChoice.Sand to Palette(
            0xFF7C580D,
            "fff8f3 201b13 4e4539 7c580d ffffff ffdeaa 271900 f8dfbb 261904 d0ebc0 0c2006 fef2e5 f8ecdf f2e6d9 807667 d2c5b4 353027 fbefe2 efbf6d ba1a1a",
            "17130b ece1d4 d2c5b4 efbf6d 422c00 5f4100 ffdeaa 54442a f8dfbb 374d2d d0ebc0 201b13 241f17 2f2921 9a8f80 4e4539 ece1d4 353027 7c580d ffb4ab",
        ),
        ThemeChoice.Rose to Palette(
            0xFF8B4A61,
            "fff8f8 22191c 514347 8b4a61 ffffff ffd9e3 3a071e ffd9e3 2b151c ffdcc2 2e1500 fff0f2 faeaed f5e4e7 837377 d5c2c6 372e30 fdedf0 ffb0c9 ba1a1a",
            "191113 efdfe1 d5c2c6 ffb0c9 541d33 6f3349 ffd9e3 5a3f47 ffd9e3 623f20 ffdcc2 22191c 261d20 31282a 9e8c90 514347 efdfe1 372e30 8b4a61 ffb4ab",
        ),
        ThemeChoice.Sage to Palette(
            0xFF38693C,
            "f7fbf2 181d18 424940 38693c ffffff b9f0b7 002106 d4e8d0 101f10 bcebf2 001f23 f1f5ec ebefe6 e6e9e1 72796f c2c9bd 2d322c eef2e9 9ed49d ba1a1a",
            "101410 e0e4db c2c9bd 9ed49d 043912 205026 b9f0b7 3a4b39 d4e8d0 1f4d53 bcebf2 181d18 1c211b 272b26 8c9389 424940 e0e4db 2d322c 38693c ffb4ab",
        ),
        ThemeChoice.Graphite to Palette(
            0xFF5C5D6C,
            "fcf8fa 1c1b1d 474648 5c5d6c ffffff e1e1f3 191b27 e2e1ec 1a1b23 dfe1f9 171b2c f6f3f4 f0edef ebe7e9 787678 c8c6c7 313032 f3f0f1 c4c5d6 ba1a1a",
            "131315 e5e1e3 c8c6c7 c4c5d6 2d303d 444654 e1e1f3 45464f e2e1ec 424659 dfe1f9 1c1b1d 201f21 2a2a2b 929092 474648 e5e1e3 313032 5c5d6c ffb4ab",
        ),
    )

    /** The colours of one line as opaque ARGB numbers, by role name. */
    fun parse(line: String): Map<String, Long> {
        val values = line.trim().split(' ')
        require(values.size == ROLES.size) { "a palette line needs ${ROLES.size} colours, got ${values.size}" }
        return ROLES.zip(values.map { 0xFF000000 or it.toLong(16) }).toMap()
    }
}
