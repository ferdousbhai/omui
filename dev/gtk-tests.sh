#!/usr/bin/env bash
set -Eeuo pipefail

# The tests that need GTK running (src/tests.rs), on a broadway display of their own with a private HOME and XDG
# directories and no session bus: nothing is shown, and nothing of the desktop this is run from is read.
#
#   dev/gtk-tests.sh [cargo test arguments]
here=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
command -v gtk4-broadwayd >/dev/null || { echo "gtk-tests: gtk4-broadwayd is required" >&2; exit 1; }

root=$(mktemp -d /tmp/omui-gtk-tests.XXXXXX)
broadway_pid=""
cleanup() {
  local status=$?
  # Only the display this script started.
  [[ -z $broadway_pid ]] || kill -TERM "$broadway_pid" 2>/dev/null || true
  [[ -z $broadway_pid ]] || wait "$broadway_pid" 2>/dev/null || true
  rm -rf -- "$root"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$root"/{home,config,state,data,cache,runtime}
chmod 700 "$root/runtime"

# A display number from this script's PID, so that two runs at once do not share one.
display=$(( 100 + $$ % 800 ))
XDG_RUNTIME_DIR=$root/runtime gtk4-broadwayd ":$display" >"$root/broadwayd.log" 2>&1 &
broadway_pid=$!
for _ in {1..50}; do
  [[ -S $root/runtime/broadway$((display + 1)).socket ]] && break
  kill -0 "$broadway_pid" 2>/dev/null || { cat "$root/broadwayd.log" >&2; exit 1; }
  sleep 0.1
done

cd "$here/.."
# Cargo and rustup keep their own homes under the real one.
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
# GTK_A11Y=test: what a widget tells a screen reader is kept for the tests to read, and no accessibility bus is sought.
env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u GTK_THEME \
  HOME="$root/home" XDG_CONFIG_HOME="$root/config" XDG_STATE_HOME="$root/state" XDG_DATA_HOME="$root/data" \
  XDG_CACHE_HOME="$root/cache" XDG_RUNTIME_DIR="$root/runtime" GTK_A11Y=test \
  GDK_BACKEND=broadway BROADWAY_DISPLAY=":$display" \
  cargo test --all-features "$@" -- --ignored
