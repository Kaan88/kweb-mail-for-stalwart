package ovh.kweb.mail.ui.contacts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Contact
import uniffi.kweb_core.Labeled

class ContactFormsTest {
    private val stored = Contact(
        addressBookId = "ab1",
        name = "Doc Ayşe",
        givenName = "Ayşe",
        familyName = "Demir",
        organization = "School",
        emails = listOf(Labeled("work", "ayse@example.org"), Labeled("", "ayse2@example.org")),
        phones = listOf(Labeled("private", "+90 312 555 01 42")),
        addresses = listOf(Labeled("", "Atatürk Bulvarı 12\nAnkara")),
        notes = "Likes tea",
    )

    @Test fun aStoredContactComesBackAsItWas() = assertEquals(stored, ContactForms.toContact(ContactForms.fromContact(stored)))

    @Test fun anUntouchedNameIsSentAsLoaded() {
        val form = ContactForms.fromContact(stored).copy(organization = "University")
        assertEquals("Doc Ayşe", ContactForms.toContact(form).name)
    }

    @Test fun aTouchedNameIsGivenAndFamily() {
        val form = ContactForms.fromContact(stored).copy(givenName = "Ayse")
        assertEquals("Ayse Demir", ContactForms.toContact(form).name)
    }

    @Test fun aTouchedFamilyNameCounts() {
        val form = ContactForms.fromContact(stored).copy(familyName = "Kaya")
        assertEquals("Ayşe Kaya", ContactForms.toContact(form).name)
    }

    @Test fun aNewContactsNameIsGivenAndFamilyTrimmed() {
        val form = ContactForms.blank("ab1").copy(givenName = " Selin ", familyName = " Kaya ")
        assertEquals("Selin Kaya", ContactForms.toContact(form).name)
    }

    @Test fun onlyOneNameFieldLeavesNoStraySpace() {
        assertEquals("Kaya", ContactForms.toContact(ContactForms.blank("ab1").copy(familyName = "Kaya")).name)
        assertEquals("Selin", ContactForms.toContact(ContactForms.blank("ab1").copy(givenName = "Selin")).name)
    }

    @Test fun clearingBothNameFieldsOfAStoredContactSendsNoName() {
        val form = ContactForms.fromContact(stored).copy(givenName = "", familyName = "")
        assertEquals("", ContactForms.toContact(form).name)
    }

    @Test fun linesWithoutAValueAreLeftOut() {
        val form = ContactForms.blank("ab1").copy(
            emails = listOf(FormRow(0, "work", "  "), FormRow(1, "work", " a@example.org ")),
            phones = listOf(FormRow(0, "", "")),
        )
        val contact = ContactForms.toContact(form)
        assertEquals(listOf(Labeled("work", "a@example.org")), contact.emails)
        assertEquals(emptyList<Labeled>(), contact.phones)
    }

    @Test fun aNewFormHasOneEmptyMailLine() = assertEquals(listOf(FormRow(0, "", "")), ContactForms.blank("ab1").emails)

    @Test fun addingALineGivesItAFreshId() {
        val form = ContactForms.removeRow(ContactForms.fromContact(stored), FormList.EMAILS, 0)
        val added = ContactForms.addRow(form, FormList.EMAILS)
        assertEquals(listOf(1, 2), ContactForms.rowsOf(added, FormList.EMAILS).map { it.id })
    }

    @Test fun removingALineKeepsTheOthers() {
        val form = ContactForms.removeRow(ContactForms.fromContact(stored), FormList.EMAILS, 0)
        assertEquals(listOf(Labeled("", "ayse2@example.org")), ContactForms.toContact(form).emails)
    }

    @Test fun changingALineChangesOnlyThatLine() {
        val form = ContactForms.changeRow(ContactForms.fromContact(stored), FormList.PHONES, 0) { it.copy(label = "work", value = "123") }
        assertEquals(listOf(Labeled("work", "123")), ContactForms.toContact(form).phones)
        assertEquals(stored.emails, ContactForms.toContact(form).emails)
    }

    @Test fun anUntouchedFormIsNotChanged() {
        val form = ContactForms.fromContact(stored)
        assertFalse(ContactForms.isChanged(form, form))
    }

    @Test fun anEmptyLineThatWasAddedIsNotAChange() {
        val form = ContactForms.fromContact(stored)
        assertFalse(ContactForms.isChanged(ContactForms.addRow(form, FormList.PHONES), form))
    }

    @Test fun typingAndTypingBackIsNotAChange() {
        val form = ContactForms.fromContact(stored)
        assertFalse(ContactForms.isChanged(form.copy(notes = "Likes tea "), form))
    }

    @Test fun anotherAddressBookIsAChange() {
        val form = ContactForms.fromContact(stored)
        assertTrue(ContactForms.isChanged(form.copy(addressBookId = "ab2"), form))
    }

    @Test fun aTypedNameIsAChangeOnANewContact() {
        val form = ContactForms.blank("ab1")
        assertTrue(ContactForms.isChanged(form.copy(givenName = "A"), form))
    }

    @Test fun aStrayBlankInANameFieldIsNotATouchedName() {
        val card = stored.copy(name = "Dr. Ayşe Luisa Demir")
        val form = ContactForms.fromContact(card)
        val typed = form.copy(givenName = form.givenName + " ")
        assertEquals("Dr. Ayşe Luisa Demir", ContactForms.name(typed))
        assertFalse(ContactForms.isChanged(typed, form))
    }

    @Test fun mergingNothingKeepsTheCurrentCard() {
        val current = stored.copy(emails = stored.emails + Labeled("", "new@example.org"), notes = "Changed elsewhere")
        assertEquals(current, ContactForms.merge(stored, stored, current))
    }

    @Test fun aChangedNoteDoesNotDeleteAnAddressAddedElsewhere() {
        val current = stored.copy(emails = stored.emails + Labeled("", "new@example.org"))
        val wanted = stored.copy(notes = "Likes coffee")
        assertEquals(current.copy(notes = "Likes coffee"), ContactForms.merge(stored, wanted, current))
    }

    @Test fun aRemovedAndAnAddedLineAreAppliedToTheCurrentList() {
        val elsewhere = Labeled("", "new@example.org")
        val current = stored.copy(emails = stored.emails + elsewhere)
        val added = Labeled("work", "second@example.org")
        val wanted = stored.copy(emails = listOf(stored.emails[1], added))
        assertEquals(listOf(stored.emails[1], elsewhere, added), ContactForms.merge(stored, wanted, current).emails)
    }

    @Test fun aLineRemovedElsewhereStaysRemoved() {
        val current = stored.copy(emails = listOf(stored.emails[1]))
        val wanted = stored.copy(phones = emptyList())
        val merged = ContactForms.merge(stored, wanted, current)
        assertEquals(current.emails, merged.emails)
        assertEquals(emptyList<Labeled>(), merged.phones)
    }

    @Test fun anUntouchedNameFollowsTheCurrentCardAndATouchedOneWins() {
        val current = stored.copy(name = "Ayşe D.")
        assertEquals("Ayşe D.", ContactForms.merge(stored, stored.copy(notes = "x"), current).name)
        assertEquals("Ayşe Yılmaz", ContactForms.merge(stored, stored.copy(name = "Ayşe Yılmaz", familyName = "Yılmaz"), current).name)
    }
}
