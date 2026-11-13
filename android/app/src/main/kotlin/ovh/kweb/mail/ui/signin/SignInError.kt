package ovh.kweb.mail.ui.signin

import uniffi.kweb_core.Exception as CoreException

/** Why a sign-in did not work, in the terms the user can act on. */
sealed interface SignInError {
    data object InvalidAddress : SignInError
    data object NoOAuth : SignInError
    data object Unreachable : SignInError
    data object NoBrowserAnswer : SignInError
    data object Refused : SignInError
    data object NoBrowser : SignInError
    data class Other(val detail: String) : SignInError

    companion object {
        /**
         * Sorts a failure of `beginSignIn`. The core reports "no OAuth here" as an Invalid error
         * from the discovery step, the same type as a malformed address, so the text tells them
         * apart (see `auth/mod.rs`).
         */
        fun fromBegin(e: CoreException): SignInError = when (e) {
            is CoreException.Invalid -> if (mentionsOAuth(e.msg)) NoOAuth else InvalidAddress
            is CoreException.Temporary -> Unreachable
            is CoreException.SignInNeeded -> Refused
            is CoreException.Store -> Other(e.msg)
        }

        /** Sorts a failure of `finishSignIn`. */
        fun fromFinish(e: CoreException): SignInError = when (e) {
            is CoreException.Temporary ->
                if (e.msg.contains("no answer from the browser", ignoreCase = true)) NoBrowserAnswer else Unreachable
            is CoreException.SignInNeeded, is CoreException.Invalid -> Refused
            is CoreException.Store -> Other(e.msg)
        }

        private fun mentionsOAuth(msg: String): Boolean =
            listOf("oauth", "client registration", "endpoint").any { msg.contains(it, ignoreCase = true) }
    }
}
