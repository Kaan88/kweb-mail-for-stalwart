package ovh.kweb.mail.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.unit.IntOffset
import org.junit.Assert.assertEquals
import org.junit.Test

class PressPointTest {
    @Test fun aFingerPutsTheMenuWhereItWentDown() {
        val p = PressPoint()
        p.sized(400, 120, rtl = false)
        p.pressed(Offset(300.4f, 55.6f))
        assertEquals(IntOffset(300, 56), p.at)
    }

    @Test fun aLongPressWithAFingerKeepsItsPoint() {
        val p = PressPoint()
        p.sized(400, 120, rtl = false)
        p.pressed(Offset(300f, 50f))
        p.settle()
        assertEquals(IntOffset(300, 50), p.at)
    }

    @Test fun aLongPressWithoutAFingerHangsBelowTheStartOfTheElement() {
        val p = PressPoint()
        p.sized(400, 120, rtl = false)
        p.pressed(Offset(300f, 50f))
        p.released()
        p.settle()
        assertEquals(IntOffset(0, 120), p.at)
    }

    @Test fun neverTouchedItAlsoHangsBelowTheStart() {
        val p = PressPoint()
        p.sized(400, 88, rtl = false)
        p.settle()
        assertEquals(IntOffset(0, 88), p.at)
    }

    @Test fun theNextTouchTakesTheMenuBackToTheFinger() {
        val p = PressPoint()
        p.sized(400, 120, rtl = false)
        p.underStart()
        p.pressed(Offset(10f, 20f))
        assertEquals(IntOffset(10, 20), p.at)
    }

    @Test fun rightToLeftTheStartIsTheRightEdgeAndTheFingerIsStillCountedFromTheLeft() {
        val p = PressPoint()
        p.sized(400, 120, rtl = true)
        p.settle()
        assertEquals(IntOffset(400, 120), p.at)
        p.pressed(Offset(30f, 40f))
        assertEquals(IntOffset(30, 40), p.at)
    }
}
