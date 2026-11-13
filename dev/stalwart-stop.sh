#!/usr/bin/env bash
# Stops the server that dev/stalwart-run.sh started (by pid, never by name).
ROOT="${KWEB_TOOLS:-$HOME/tools}/stalwart-test"
if [ -f "$ROOT/etc/stalwart.pid" ]; then
  pid="$(cat "$ROOT/etc/stalwart.pid")"
  kill "$pid" 2>/dev/null && while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
  rm -f "$ROOT/etc/stalwart.pid"
fi
echo stopped
