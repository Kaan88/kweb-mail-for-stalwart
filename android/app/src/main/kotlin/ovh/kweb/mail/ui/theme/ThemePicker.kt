package ovh.kweb.mail.ui.theme

import ovh.kweb.mail.R

/** The name of a colour choice, as the Appearance screen and the Settings row show it. */
fun themeName(choice: ThemeChoice): Int = when (choice) {
    ThemeChoice.Wallpaper -> R.string.theme_wallpaper
    ThemeChoice.Ward -> R.string.theme_ward
    ThemeChoice.Lavender -> R.string.theme_lavender
    ThemeChoice.Moss -> R.string.theme_moss
    ThemeChoice.Peach -> R.string.theme_peach
    ThemeChoice.Ocean -> R.string.theme_ocean
    ThemeChoice.Slate -> R.string.theme_slate
    ThemeChoice.Sand -> R.string.theme_sand
    ThemeChoice.Rose -> R.string.theme_rose
    ThemeChoice.Sage -> R.string.theme_sage
    ThemeChoice.Graphite -> R.string.theme_graphite
}

/** The name of a mode. */
fun themeModeName(mode: ThemeMode): Int = when (mode) {
    ThemeMode.System -> R.string.appearance_mode_system
    ThemeMode.Light -> R.string.appearance_mode_light
    ThemeMode.Dark -> R.string.appearance_mode_dark
}
