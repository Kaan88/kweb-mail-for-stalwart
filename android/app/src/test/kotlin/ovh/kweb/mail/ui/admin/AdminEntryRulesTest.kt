package ovh.kweb.mail.ui.admin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import ovh.kweb.mail.R

class AdminEntryRulesTest {
    @Test fun noAcceptedKeyMeansNoEntry() {
        assertNull(AdminEntryRules.entry(false, false))
        assertNull(AdminEntryRules.entry(false, true))
    }

    @Test fun aKeyThatOpensTheDashboardGoesStraightToIt() {
        assertEquals(AdminEntry.Dashboard, AdminEntryRules.entry(true, true))
        assertEquals(R.string.admin_drawer_dashboard, AdminEntryRules.label(AdminEntry.Dashboard))
    }

    @Test fun otherwiseTheEntryIsTheAdministrationHome() {
        assertEquals(AdminEntry.Administration, AdminEntryRules.entry(true, false))
        assertEquals(R.string.admin_title, AdminEntryRules.label(AdminEntry.Administration))
    }
}
