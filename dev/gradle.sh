#!/usr/bin/env bash
# Runs the Gradle wrapper of android/ with the development environment and build output on the
# home drive:   dev/gradle.sh assembleDebug      dev/gradle.sh lintDebug testDebugUnitTest
# The APK ends up in ~/.cache/kweb-mail/gradle/app/outputs/apk/debug/.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=env.sh
source "$HERE/env.sh"
cd "$HERE/../android"
exec sh gradlew -I "$HERE/gradle-linux.init.gradle" "$@"
