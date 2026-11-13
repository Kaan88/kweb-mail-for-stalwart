package ovh.kweb.mail.ui.inbox

import java.time.ZoneId
import java.time.ZonedDateTime
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RowFormatTest {
    private val zone = ZoneId.of("UTC")
    private val locale = Locale.UK

    /** Friday 13 November 2026, 12:00. */
    private val now = ZonedDateTime.of(2026, 11, 13, 12, 0, 0, 0, zone).toInstant().toEpochMilli()

    private fun at(day: Int, hour: Int, minute: Int = 0, month: Int = 11, year: Int = 2026): Long =
        ZonedDateTime.of(year, month, day, hour, minute, 0, 0, zone).toEpochSecond()

    private fun format(received: Long) = RowTime.format(received, now, zone, locale, "Yesterday")

    @Test fun todayShowsTheTime() = assertTrue(format(at(13, 9, 38)).contains("09:38"))

    @Test fun justAfterMidnightIsStillToday() = assertTrue(format(at(13, 0, 1)).contains("00:01"))

    @Test fun yesterdayIsNamed() = assertEquals("Yesterday", format(at(12, 23, 59)))

    @Test fun withinAWeekShowsTheWeekday() {
        assertEquals("Wed", format(at(11, 8)))
        assertEquals("Sat", format(at(7, 8)))
    }

    @Test fun sevenDaysAgoShowsTheDate() = assertTrue(format(at(6, 8)).contains("6 Nov"))

    @Test fun olderMailShowsTheYearToo() = assertTrue(format(at(3, 8, month = 2, year = 2025)).contains("2025"))

    @Test fun aTimeInTheFutureCountsAsToday() = assertTrue(format(at(14, 9, 38)).contains("09:38"))

    @Test fun theLocalZoneDecidesWhereTheDayStarts() {
        val tokyo = ZoneId.of("Asia/Tokyo")
        // 20:00 UTC on the 12th is 05:00 on the 13th in Tokyo, which is "now" there too.
        val received = ZonedDateTime.of(2026, 11, 12, 20, 0, 0, 0, ZoneId.of("UTC")).toEpochSecond()
        assertTrue(RowTime.format(received, now, tokyo, locale, "Yesterday").contains("05:00"))
    }

    @Test fun senderPrefersTheName() = assertEquals("Merve", Sender.label("Merve", "merve@example.org"))

    @Test fun senderFallsBackToTheAddress() = assertEquals("a@b.c", Sender.label("  ", "a@b.c"))

    @Test fun noSenderAtAll() = assertNull(Sender.label("", " "))

    @Test fun initialIsUpperCase() = assertEquals("M", Sender.initial("merve", "x@y.z"))

    @Test fun initialKeepsNonAsciiLetters() = assertEquals("Ş", Sender.initial("Şule", "x@y.z"))

    @Test fun initialSkipsQuotesAndPunctuation() = assertEquals("H", Sender.initial("\"<Hetzner>\"", ""))

    @Test fun initialOfAnAddress() = assertEquals("S", Sender.initial("", "selin@example.org"))

    @Test fun initialOfNothingIsAQuestionMark() = assertEquals("?", Sender.initial("", ""))

    @Test fun initialDoesNotSplitASurrogatePair() = assertEquals("𝐀", Sender.initial("𝐀bc", ""))
}
