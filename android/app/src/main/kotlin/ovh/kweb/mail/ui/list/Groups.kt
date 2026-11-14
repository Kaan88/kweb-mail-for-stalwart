package ovh.kweb.mail.ui.list

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.dp

/**
 * Rows that belong together are drawn as one group (Material 3 Expressive): a small gap between
 * them, small corners inside, and large corners where the group begins and ends.
 */
object Groups {
    const val OUTER_DP = 28
    const val INNER_DP = 8

    /** The corner radius in dp at the top and at the bottom of row [index] in a group of [count]. */
    fun corners(index: Int, count: Int): Pair<Int, Int> =
        (if (index <= 0) OUTER_DP else INNER_DP) to (if (index >= count - 1) OUTER_DP else INNER_DP)

    fun shape(index: Int, count: Int): Shape {
        val (top, bottom) = corners(index, count)
        return RoundedCornerShape(topStart = top.dp, topEnd = top.dp, bottomEnd = bottom.dp, bottomStart = bottom.dp)
    }
}
