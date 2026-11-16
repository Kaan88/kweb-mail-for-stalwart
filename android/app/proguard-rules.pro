# The Rust core is reached through UniFFI bindings over JNA; JNA looks things up by reflection.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-dontwarn java.awt.**
-dontwarn com.sun.jna.**

# Generated UniFFI bindings: structures, callback interfaces, the library interface.
-keep class uniffi.** { *; }
-keepclassmembers class uniffi.** { *; }

# Native side calls these by name (Java_ovh_kweb_mail_Tls_init) and rustls-platform-verifier by reflection.
-keep class ovh.kweb.mail.Tls { *; }
-keep class org.rustls.platformverifier.** { *; }

# ViewModels are created by reflection through their constructors.
-keep class * extends androidx.lifecycle.ViewModel { <init>(...); }
-keep class * extends androidx.lifecycle.AndroidViewModel { <init>(...); }
-keep class * extends androidx.work.ListenableWorker { <init>(...); }

# Readable stack traces from a phone.
-keepattributes SourceFile,LineNumberTable
