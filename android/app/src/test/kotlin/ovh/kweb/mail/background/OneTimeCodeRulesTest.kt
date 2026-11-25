package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.background.MailAction.Archive
import ovh.kweb.mail.background.MailAction.Delete
import ovh.kweb.mail.background.MailAction.MarkRead
import ovh.kweb.mail.background.MailAction.Reply

class OneTimeCodeRulesTest {
    private fun find(subject: String, preview: String = "") = OneTimeCodeRules.find(subject, preview)

    @Test
    fun aNumberAfterNumberSignOrBookingIsNotACode() {
        org.junit.Assert.assertNull(find("Confirmation #5829104"))
        org.junit.Assert.assertNull(find("Verification: booking 998877"))
    }

    @Test
    fun aCodeNextToAnEnglishCue() {
        assertEquals("482915", find("Your verification code", "Your code is 482915. It expires in 10 minutes."))
        assertEquals("482915", find("482915 is your login code"))
        assertEquals("123456", find("G-123456 is your Google verification code"))
    }

    @Test
    fun groupedCodesLoseTheirSeparator() {
        assertEquals("123456", find("Security code", "Use 123 456 to sign in"))
        assertEquals("123456", find("Security code", "Use 123-456 to sign in"))
        assertEquals("12345678", find("Passcode", "Your passcode is 1234 5678"))
    }

    @Test
    fun otherLanguages() {
        assertEquals("739201", find("Doğrulama kodunuz", "Doğrulama kodunuz: 739201"))
        assertEquals("739201", find("Giriş için tek kullanımlık şifre 739201"))
        assertEquals("558120", find("Ihr Bestätigungscode", "Ihr Bestätigungscode lautet 558120"))
        assertEquals("558120", find("Votre code de vérification est 558120"))
        assertEquals("558120", find("Tu código de verificación es 558120"))
        assertEquals("558120", find("Ваш код подтверждения 558120"))
        assertEquals("558120", find("您的验证码558120，请勿泄露"))
        assertEquals("558120", find("Kod: 558120"))
    }

    @Test
    fun aMixedLettersAndDigitsCodeNearACue() {
        assertEquals("A1B2C3", find("Your login code", "Enter A1B2C3 to continue"))
        assertEquals("K7X9Q2", find("Verification", "Code: K7X9Q2"))
    }

    @Test
    fun aWordOrAPlainNumberWithoutACueIsNotACode() {
        assertNull(find("Lunch tomorrow?", "How about 12:30 at the usual place, table 4821"))
        assertNull(find("Your newsletter", "Issue 482915 is out"))
        assertNull(find("Hello", "HELLO WORLD"))
    }

    @Test
    fun aMixedWordNeedsBothLettersAndDigits() {
        assertNull(find("Your login code", "Enter ABCDEF to continue"))
    }

    @Test
    fun yearsAreRejectedUnlessTheCueIsRightThere() {
        assertNull(find("Verification code policy update for everyone", "Since 2024 we have changed how we protect your files"))
        assertEquals("2024", find("Your code is 2024"))
    }

    @Test
    fun phoneNumbersAreRejected() {
        assertNull(find("Verification needed", "Call +90 555 123 4567 to verify your account"))
        assertNull(find("Security code", "Phone: 555 123 4567"))
        assertNull(find("Verification", "Tel 5551234 for help"))
    }

    @Test
    fun amountsAndDurationsAreRejected() {
        assertNull(find("Verification code sent", "Your payment of $1500 was received"))
        assertNull(find("Verification code sent", "Your payment of 1500 TL was received"))
        assertNull(find("Login code", "Valid for 15000 seconds"))
        assertNull(find("Login code", "Discount 2500%"))
    }

    @Test
    fun orderNumbersAreRejected() {
        assertNull(find("Verification code for order", "Order 5829104 has shipped"))
        assertNull(find("Doğrulama", "Sipariş no: 5829104"))
    }

    @Test
    fun theCandidateNearestACueWins() {
        assertEquals("884211", find("Your order 1234567 has shipped", "Order 1234567. Verification code: 884211"))
        // Two numbers after the cue: the closer one.
        assertEquals("112233", find("Login", "Your code is 112233, reference number 998877 for support"))
    }

    @Test
    fun aNumberNextToAnotherNumberIsNotACode() {
        assertNull(find("Verification", "Card 1234 5678 9012 3456"))
        assertNull(find("Verification code", "Date 2024-05-12"))
    }

    @Test
    fun nothingWithoutACue() {
        assertNull(find("", ""))
        assertNull(find("Meeting", "Room 4821, floor 3"))
    }

    @Test
    fun theCopyButtonGoesFirstAndTheLastOneMakesRoomWhenThreeAreChosen() {
        assertEquals(
            OneTimeCodeRules.ButtonPlan(true, listOf(Delete, MarkRead)),
            OneTimeCodeRules.plan(listOf(Delete, MarkRead, Reply), true),
        )
        assertEquals(
            OneTimeCodeRules.ButtonPlan(true, listOf(Archive, Reply)),
            OneTimeCodeRules.plan(listOf(Archive, Reply), true),
        )
        assertEquals(OneTimeCodeRules.ButtonPlan(true, emptyList()), OneTimeCodeRules.plan(emptyList(), true))
    }

    @Test
    fun withoutACodeTheChoiceIsUntouched() {
        val plan = OneTimeCodeRules.plan(listOf(Delete, MarkRead, Reply), false)
        assertFalse(plan.copy)
        assertEquals(listOf(Delete, MarkRead, Reply), plan.actions)
        assertTrue(OneTimeCodeRules.plan(emptyList(), true).copy)
    }

    @Test
    fun aVeryLongSubjectIsCutBeforeTheScan() {
        val subject = "x ".repeat(400) + "verification code 482915"
        org.junit.Assert.assertNull(find(subject))
        assertEquals("482915", find("Your verification code 482915 " + "x".repeat(2000)))
    }
}
