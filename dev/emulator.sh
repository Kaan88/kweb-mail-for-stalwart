#!/usr/bin/env bash
# Starts or stops the Android emulator "kweb35" (Android 15, x86_64) and maps the test server's
# port into it, so the app can sign in to http://127.0.0.1:18080 from inside the emulator.
#
#   dev/emulator.sh            start without a window (screenshots through adb)
#   dev/emulator.sh window     start with a window on the desktop
#   dev/emulator.sh stop
#
# KWEB_EMU_ACCEL=off starts it without KVM (slow: several minutes to boot), for a machine where
# KVM cannot be used. It runs beside the owner's VirtualBox VMs with KVM (owner, 2026-11-14).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=env.sh
source "$HERE/env.sh"
LOG="${KWEB_TOOLS:-$HOME/tools}/emulator.log"

case "${1:-start}" in
  stop)
    adb emu kill >/dev/null 2>&1 || true   # the emulator's own console; no other process is touched
    echo stopped; exit 0 ;;
  start|window) ;;
  *) echo "usage: $0 [start|window|stop]" >&2; exit 2 ;;
esac

if ! adb devices | grep -q "^emulator-.*device$"; then
  flags=(-no-audio -no-snapshot -no-boot-anim -gpu swiftshader_indirect -memory 3072)
  if [ "${KWEB_EMU_ACCEL:-}" = off ]; then
    flags+=(-accel off -cores 6)
  fi
  [ "${1:-start}" = window ] || flags+=(-no-window)
  setsid nohup emulator -avd kweb35 "${flags[@]}" >"$LOG" 2>&1 </dev/null &
  adb start-server >/dev/null 2>&1 || true
  adb wait-for-device
fi
until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 1; done
adb reverse tcp:18080 tcp:18080 >/dev/null
echo "emulator ready; the emulator's 127.0.0.1:18080 is this machine's test server"
