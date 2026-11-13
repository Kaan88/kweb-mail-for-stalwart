package ovh.kweb.mail.ui.signin

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.kweb_core.Exception as CoreException

class SignInErrorTest {
    @Test fun discoveryNotFoundMeansNoOAuth() = assertEquals(
        SignInError.NoOAuth,
        SignInError.fromBegin(CoreException.Invalid("OAuth discovery: HTTP 404 Not Found (does the server support OAuth?)")),
    )

    @Test fun missingRegistrationMeansNoOAuth() = assertEquals(
        SignInError.NoOAuth,
        SignInError.fromBegin(CoreException.Invalid("the server does not offer client registration")),
    )

    @Test fun otherInvalidIsAnAddressProblem() = assertEquals(
        SignInError.InvalidAddress,
        SignInError.fromBegin(CoreException.Invalid("Mail server URL must use https")),
    )

    @Test fun temporaryOnBeginIsUnreachable() =
        assertEquals(SignInError.Unreachable, SignInError.fromBegin(CoreException.Temporary("OAuth discovery: timed out")))

    @Test fun timeoutOnFinishIsNoBrowserAnswer() = assertEquals(
        SignInError.NoBrowserAnswer,
        SignInError.fromFinish(CoreException.Temporary("no answer from the browser within 5 minutes")),
    )

    @Test fun otherTemporaryOnFinishIsUnreachable() =
        assertEquals(SignInError.Unreachable, SignInError.fromFinish(CoreException.Temporary("token request: timed out")))

    @Test fun refusedByTheServer() =
        assertEquals(SignInError.Refused, SignInError.fromFinish(CoreException.SignInNeeded("sign-in refused: access_denied")))
}
