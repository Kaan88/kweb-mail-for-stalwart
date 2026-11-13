# Source this before building or testing on the Linux development machine:
#
#     source dev/env.sh
#
# Everything it points at lives under ~/tools, outside the repository and outside the system
# packages (see dev/README.md). It changes only the current shell.

TOOLS="${KWEB_TOOLS:-$HOME/tools}"

export JAVA_HOME="$TOOLS/jdk"
export ANDROID_HOME="$TOOLS/android-sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
# The newest NDK installed under the SDK.
ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
export ANDROID_NDK_HOME

# A private Rust toolchain (rustup) with the Android targets, so the system's cargo is left alone.
export RUSTUP_HOME="$TOOLS/rust/rustup"
export CARGO_HOME="$TOOLS/rust/cargo"

# The repository is on an NTFS drive, where builds are slow; build output goes to the home drive.
export CARGO_TARGET_DIR="${KWEB_TARGET_DIR:-$HOME/.cache/kweb-mail/target}"

export PATH="$CARGO_HOME/bin:$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$TOOLS/stalwart-test/node/bin:$PATH"

unset TOOLS
