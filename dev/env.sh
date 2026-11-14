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

# Build output goes to /mnt/E when that drive is there (the root drive is nearly full; a full test
# build was tried there and passes), otherwise to the home drive. KWEB_TARGET_DIR overrides both.
if [ -d /mnt/E ]; then KWEB_DEFAULT_TARGET=/mnt/E/cargo-target/kweb-mail; else KWEB_DEFAULT_TARGET="$HOME/.cache/kweb-mail/target"; fi
export CARGO_TARGET_DIR="${KWEB_TARGET_DIR:-$KWEB_DEFAULT_TARGET}"
unset KWEB_DEFAULT_TARGET

# Builds are heavy on this machine (the linker writes hundreds of MB per second), so: four build
# jobs, four test threads, low CPU and disk priority, and only one cargo at a time across the
# repository and its copies (a lock file; a second cargo waits for the first).
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-4}"
KWEB_BUILD_LOCK="$HOME/.cache/kweb-mail/build.lock"
export KWEB_BUILD_LOCK
cargo() { flock "$KWEB_BUILD_LOCK" nice -n 15 ionice -c3 cargo "$@"; }

export PATH="$CARGO_HOME/bin:$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$TOOLS/stalwart-test/node/bin:$PATH"

unset TOOLS
