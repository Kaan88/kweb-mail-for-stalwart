package ovh.kweb.mail.ui.contacts

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.kweb_core.ContactRow
import uniffi.kweb_core.Labeled

class ContactRulesTest {
    private val en = Locale.ENGLISH
    private val tr = Locale.forLanguageTag("tr-TR")

    private fun row(id: String, name: String, email: String = "", org: String = "") = ContactRow(id, name, email, org)

    @Test fun theLetterIsTheUpperCasedFirstLetter() = assertEquals("A", ContactRules.letter("ayşe Demir", en))

    @Test fun surroundingBlanksAreIgnored() = assertEquals("D", ContactRules.letter("  deniz", en))

    @Test fun turkishDottedIUpperCasesToDottedCapital() = assertEquals("İ", ContactRules.letter("ilker", tr))

    @Test fun turkishDotlessIUpperCasesToPlainCapital() = assertEquals("I", ContactRules.letter("ısıl", tr))

    @Test fun englishIUpperCasesToPlainCapital() = assertEquals("I", ContactRules.letter("ilker", en))

    @Test fun anAccentedLetterKeepsItsAccent() = assertEquals("Ç", ContactRules.letter("çağla", tr))

    @Test fun aSharpSIsOneLetter() = assertEquals("S", ContactRules.letter("ßeta", Locale.GERMAN))

    @Test fun aDigitIsUnderTheOtherHeading() = assertEquals("#", ContactRules.letter("1password", en))

    @Test fun aSymbolIsUnderTheOtherHeading() = assertEquals("#", ContactRules.letter("+90 312", en))

    @Test fun anEmptyNameIsUnderTheOtherHeading() = assertEquals("#", ContactRules.letter("   ", en))

    @Test fun aLetterBeyondTheBasicPlaneIsNotSplit() = assertEquals("𐐀", ContactRules.letter("𐐨x", en))

    @Test fun theInitialSkipsLeadingSymbols() = assertEquals("O", ContactRules.initial("\"ozan\"", en))

    @Test fun theInitialMayBeADigit() = assertEquals("4", ContactRules.initial("4 Seasons", en))

    @Test fun theInitialOfNothingIsAQuestionMark() = assertEquals("?", ContactRules.initial("--", en))

    @Test fun theInitialFollowsTheLocale() = assertEquals("İ", ContactRules.initial("ipek", tr))

    @Test fun contactsAreGroupedUnderTheirLetters() {
        val entries = ContactRules.group(listOf(row("1", "Ayşe"), row("2", "Ali"), row("3", "Deniz")), en)
        assertEquals(
            listOf(
                ContactRules.Entry.Heading("A"),
                ContactRules.Entry.Item(row("1", "Ayşe"), 0, 2),
                ContactRules.Entry.Item(row("2", "Ali"), 1, 2),
                ContactRules.Entry.Heading("D"),
                ContactRules.Entry.Item(row("3", "Deniz"), 0, 1),
            ),
            entries,
        )
    }

    @Test fun theOtherHeadingComesLastEvenWhenSuchNamesCameFirst() {
        val entries = ContactRules.group(listOf(row("1", "1st"), row("2", "Ali"), row("3", "(x)")), en)
        assertEquals(listOf("A", "#"), entries.filterIsInstance<ContactRules.Entry.Heading>().map { it.letter })
        assertEquals(2, (entries.last() as ContactRules.Entry.Item).groupSize)
    }

    @Test fun aLetterHasOneHeadingEvenWhenItsContactsAreApart() {
        val entries = ContactRules.group(listOf(row("1", "Ali"), row("2", "Bora"), row("3", "ali2")), en)
        assertEquals(listOf("A", "B"), entries.filterIsInstance<ContactRules.Entry.Heading>().map { it.letter })
    }

    @Test fun noContactsNoEntries() = assertEquals(emptyList<ContactRules.Entry>(), ContactRules.group(emptyList(), en))

    @Test fun theQuietLineHasAddressAndOrganization() =
        assertEquals("a@example.org · School", ContactRules.subtitle(row("1", "A", "a@example.org", "School")))

    @Test fun theQuietLineSkipsWhatIsMissing() {
        assertEquals("School", ContactRules.subtitle(row("1", "A", "", " School ")))
        assertEquals("a@example.org", ContactRules.subtitle(row("1", "A", "a@example.org", "")))
        assertEquals("", ContactRules.subtitle(row("1", "A")))
    }

    @Test fun theFirstAddressWithAValueIsTheOneForRecentMail() =
        assertEquals("b@example.org", ContactRules.firstEmail(listOf(Labeled("", "  "), Labeled("work", " b@example.org "))))

    @Test fun noAddressNoRecentMail() = assertEquals(null, ContactRules.firstEmail(listOf(Labeled("", ""))))

    @Test fun aNumberIsDialledWithoutSpacesAndPunctuation() = assertEquals("tel:%2B903125550142", ContactRules.dialUri("+90 (312) 555-01 42"))

    @Test fun aNumberWithNothingDiallableIsLeftAlone() = assertEquals("ext", ContactRules.dialTarget(" ext "))

    @Test fun anAddressBecomesAMapQuery() =
        assertEquals("geo:0,0?q=Atat%C3%BCrk%20Bulvar%C4%B1%2012%2C%20Ankara", ContactRules.geoUri("Atatürk Bulvarı 12\nAnkara"))

    @Test fun theLabelsOfTheCoreAreWorkAndPrivate() {
        assertEquals(ContactRules.LabelKind.WORK, ContactRules.labelKind("work"))
        assertEquals(ContactRules.LabelKind.WORK, ContactRules.labelKind(" Work "))
        assertEquals(ContactRules.LabelKind.PRIVATE, ContactRules.labelKind("private"))
        assertEquals(ContactRules.LabelKind.OTHER, ContactRules.labelKind(""))
    }

    @Test fun aWordOfTheUsersOwnIsKept() {
        assertEquals(ContactRules.LabelKind.CUSTOM, ContactRules.labelKind("Mobile"))
        assertEquals("Mobile", ContactRules.labelValue(ContactRules.LabelKind.CUSTOM, "Mobile"))
    }

    @Test fun theMenuStoresTheCoresWords() {
        assertEquals("work", ContactRules.labelValue(ContactRules.LabelKind.WORK, "Mobile"))
        assertEquals("private", ContactRules.labelValue(ContactRules.LabelKind.PRIVATE, ""))
        assertEquals("", ContactRules.labelValue(ContactRules.LabelKind.OTHER, "work"))
    }

    @Test fun aNameThatIsAnotherAddressOfTheCardIsNoName() {
        val emails = listOf(Labeled("", "a@work.test"), Labeled("", "b@home.test"))
        assertEquals("", ContactRules.recipientName("a@work.test", emails, "b@home.test"))
        assertEquals("", ContactRules.recipientName("b@home.test", emails, "b@home.test"))
        assertEquals("Anna Keller", ContactRules.recipientName("Anna Keller", emails, "b@home.test"))
    }
}
