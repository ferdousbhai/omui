#!/usr/bin/env bash
set -Eeuo pipefail

# Runs an app built with omui's feature `qa` through a QA script, on a broadway display of its own with a private
# HOME and XDG directories and no session bus: no window opens anywhere visible, and nothing of the desktop this is
# run from is read. The app's own stdout and stderr pass through; its answers come on descriptor 3 (OMUI_QA_FD).
# Prints each command of the script with its answer, and exits with the app's status: non-zero when a command
# failed or GLib logged a warning or a critical. The protocol is in README.md.
#
#   dev/qa.sh <app-binary> <script.jsonl>
#   OMUI_QA_TIMEOUT=120                    seconds before the whole run is stopped
(( $# == 2 )) || { echo "usage: dev/qa.sh <app-binary> <script.jsonl>" >&2; exit 2; }
app=$(realpath -- "$1")
script=$(realpath -- "$2")
command -v gtk4-broadwayd >/dev/null || { echo "qa: gtk4-broadwayd is required" >&2; exit 1; }

root=$(mktemp -d /tmp/omui-qa.XXXXXX)
broadway_pid=""
app_pid=""
cleanup() {
  local status=$?
  # Only the app and the display this script started.
  [[ -z $app_pid ]] || kill -TERM "$app_pid" 2>/dev/null || true
  [[ -z $app_pid ]] || wait "$app_pid" 2>/dev/null || true
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

# A display number and input port from this script's PID, so that two runs at once do not share one. For the run,
# broadwayd listens on that TCP port on 127.0.0.1 alone (not on another interface), where the app connects to send
# key presses (`key`, `type`) as broadway's web page would.
display=$(( 100 + $$ % 800 ))
port=$(( 8080 + display ))
XDG_RUNTIME_DIR=$root/runtime gtk4-broadwayd --address 127.0.0.1 --port "$port" ":$display" \
  >"$root/broadwayd.log" 2>&1 &
broadway_pid=$!
for _ in {1..50}; do
  [[ -S $root/runtime/broadway$((display + 1)).socket ]] && break
  kill -0 "$broadway_pid" 2>/dev/null || { cat "$root/broadwayd.log" >&2; exit 1; }
  sleep 0.1
done

# OMUI_QA_SCRIPT names the script, so that the paths it gives (a catalog, a picker's files) are read beside it.
# In the background, waited for: bash runs a trap only once the foreground command ends, and `timeout` puts the app in
# a process group of its own, which Ctrl-C does not reach. Killing `timeout` stops the app with it.
status=0
env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u GTK_THEME -u HYPRLAND_INSTANCE_SIGNATURE \
  HOME="$root/home" XDG_CONFIG_HOME="$root/config" XDG_STATE_HOME="$root/state" XDG_DATA_HOME="$root/data" \
  XDG_CACHE_HOME="$root/cache" XDG_RUNTIME_DIR="$root/runtime" \
  GDK_BACKEND=broadway BROADWAY_DISPLAY=":$display" OMUI_QA=1 OMUI_QA_INPUT="127.0.0.1:$port" OMUI_QA_FD=3 \
  OMUI_QA_SCRIPT="$script" \
  timeout "${OMUI_QA_TIMEOUT:-120}" "$app" <"$script" 3>"$root/answers" &
app_pid=$!
wait "$app_pid" || status=$?
app_pid=""
# A display that died under the app (an assertion of broadwayd's) shows in the app only as its connection lost.
kill -0 "$broadway_pid" 2>/dev/null || { echo "qa: the broadway display ended:" >&2; cat "$root/broadwayd.log" >&2; }

# Each command beside its answer; `quit`'s answer, or the failure that ended the run, comes last.
grep -Ev '^[[:space:]]*(#|$)' "$script" | awk -v answers="$root/answers" '
  { if ((getline answer < answers) > 0) { print "> " $0; print "< " answer } else { print "> " $0 " (not run)" } }
  END { while ((getline answer < answers) > 0) print "< " answer }'
(( status == 0 )) && echo "qa: passed" || echo "qa: failed with status $status"
exit "$status"
