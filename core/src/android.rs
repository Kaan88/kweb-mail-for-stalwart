//! Android only: hands the JVM to the TLS certificate verifier.
//!
//! The HTTP stack checks server certificates with Android's own trust store (so a private
//! or user-installed CA works), and that check is a call into the JVM. It needs the
//! application context once per process, before the first HTTPS connection; without it the
//! first handshake panics. UniFFI cannot pass a `Context`, so this is a plain JNI function.

use jni::{
    errors::ThrowRuntimeExAndDefault,
    jni_mangle,
    objects::{JClass, JObject},
    EnvUnowned,
};

/// Kotlin: `ovh.kweb.mail.Tls.init(context)`, a static native method. A failure is thrown
/// as a RuntimeException on the Kotlin side.
#[jni_mangle("ovh.kweb.mail.Tls")]
pub fn init<'caller>(mut unowned_env: EnvUnowned<'caller>, _class: JClass<'caller>, context: JObject<'caller>) {
    unowned_env
        .with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
        .resolve::<ThrowRuntimeExAndDefault>();
}
