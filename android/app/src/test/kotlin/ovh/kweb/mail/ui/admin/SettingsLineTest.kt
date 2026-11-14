package ovh.kweb.mail.ui.admin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import ovh.kweb.mail.R

class SettingsLineTest {
    @Test fun theSecondLineOfTheSettingsRow() {
        assertNull(settingsLine(AccessState()))
        assertEquals(R.string.admin_row_locked, settingsLine(AccessState(phase = AccessPhase.READY, access = access(connected = false, all = false))))
        assertEquals(R.string.admin_row_open, settingsLine(AccessState(phase = AccessPhase.READY, access = access())))
        assertEquals(R.string.admin_row_refused, settingsLine(AccessState(phase = AccessPhase.READY, access = access(connected = false, refused = true, all = false))))
    }
}
