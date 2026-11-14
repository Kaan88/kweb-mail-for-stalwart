package ovh.kweb.mail.ui.list

import org.junit.Assert.assertEquals
import org.junit.Test

class GroupsTest {
    @Test
    fun aGroupHasLargeCornersOnlyWhereItBeginsAndEnds() {
        assertEquals(28 to 8, Groups.corners(0, 3))
        assertEquals(8 to 8, Groups.corners(1, 3))
        assertEquals(8 to 28, Groups.corners(2, 3))
    }

    @Test
    fun aSingleRowIsRoundAllOver() {
        assertEquals(28 to 28, Groups.corners(0, 1))
    }
}
