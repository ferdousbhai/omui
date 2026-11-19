#!/usr/bin/env bash
set -Eeuo pipefail

# The gallery in a nested Hyprland with a private HOME, XDG directories and D-Bus, so nothing of the desktop it is
# run from is read or shown on: not its theme, its GTK settings, or its windows.
#
#   dev/preview.sh <screenshot.png> [theme-dir [next-theme-dir]]
#
# `theme-dir` holds a `colors.toml` (an Omarchy theme, such as /usr/share/omarchy/themes/catppuccin-latte) and is
# copied in as the scratch desktop's current theme; without one there is no Omarchy state and the gallery shows the
# fallback. With `next-theme-dir` the scratch theme directory is then replaced by `mv` while the gallery runs, as
# Omarchy does on a switch, and a second picture is written beside the first as `<screenshot>-switched.png`.
#
#   OMUI_GALLERY_PAGE, OMUI_GALLERY_OPEN, OMUI_GALLERY_TRACE   the gallery's own (examples/gallery.rs)
#   OMUI_PREVIEW_BINARY the gallery to run (default target/debug/examples/gallery)
#   OMUI_PREVIEW_SIZE   the size of the window and of the picture (default 1280x820)
#   OMUI_PREVIEW_SETTLE seconds to wait before each picture (default 4)
#   OMUI_PREVIEW_KEYS   keys to press in the gallery before the first picture, as `wtype -k` names them, each with
#                       an optional modifier ("Tab shift+Tab Return")
#   OMUI_PREVIEW_LOG    a file for what the gallery printed
if (( $# < 1 || $# > 3 )); then
  echo "usage: $0 <screenshot.png> [theme-dir [next-theme-dir]]" >&2
  exit 2
fi
screenshot=$(realpath -m -- "$1")
theme=${2:-}
next_theme=${3:-}

here=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
binary=${OMUI_PREVIEW_BINARY:-$here/../target/debug/examples/gallery}
[[ -x $binary ]] || { echo "preview: build the gallery first (cargo build --example gallery)" >&2; exit 1; }
for command_name in Hyprland hyprctl grim jq dbus-daemon setsid; do
  command -v "$command_name" >/dev/null || { echo "preview: $command_name is required" >&2; exit 1; }
done
keys=${OMUI_PREVIEW_KEYS:-}
[[ -z $keys ]] || command -v wtype >/dev/null || { echo "preview: wtype is required to press keys" >&2; exit 1; }
for dir in "$theme" "$next_theme"; do
  [[ -z $dir || -f $dir/colors.toml ]] || { echo "preview: $dir has no colors.toml" >&2; exit 1; }
done
parent_runtime=${XDG_RUNTIME_DIR:?preview: XDG_RUNTIME_DIR is required}
parent_display=${WAYLAND_DISPLAY:?preview: run this from a Wayland desktop}
parent_socket=$parent_runtime/$parent_display
settle=${OMUI_PREVIEW_SETTLE:-4}
size=${OMUI_PREVIEW_SIZE:-1280x820}
[[ $size =~ ^[0-9]+x[0-9]+$ ]] || { echo "preview: OMUI_PREVIEW_SIZE is <width>x<height>" >&2; exit 2; }

root=""
runtime=""
hypr_pid=""
gallery_pid=""
dbus_pid=""
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  local pid
  # Only the PIDs this script started.
  for pid in "$gallery_pid" "$hypr_pid" "$dbus_pid"; do
    [[ -z $pid ]] || kill -TERM "$pid" 2>/dev/null || true
  done
  for _ in {1..30}; do
    local alive=0
    for pid in "$gallery_pid" "$hypr_pid" "$dbus_pid"; do
      [[ -z $pid ]] || ! kill -0 "$pid" 2>/dev/null || alive=1
    done
    (( alive == 0 )) && break
    sleep 0.1
  done
  for pid in "$gallery_pid" "$hypr_pid" "$dbus_pid"; do
    [[ -z $pid ]] || kill -KILL "$pid" 2>/dev/null || true
  done
  [[ -z $root || $root != /tmp/omui-preview.* ]] || rm -rf -- "$root"
  [[ -z $runtime || $runtime != /tmp/omp.* ]] || rm -rf -- "$runtime"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Wayland and Hyprland IPC use AF_UNIX paths capped at 108 bytes.
root=$(mktemp -d /tmp/omui-preview.XXXXXX)
runtime=$(mktemp -d /tmp/omp.XXXXXX)
chmod 700 "$root" "$runtime"
mkdir -p "$root"/{home,config,state,data,cache}

export HOME=$root/home
export XDG_CONFIG_HOME=$root/config XDG_STATE_HOME=$root/state XDG_DATA_HOME=$root/data XDG_CACHE_HOME=$root/cache
export XDG_RUNTIME_DIR=$runtime
unset GTK_THEME

# Where Omarchy's scripts write it, whatever XDG_STATE_HOME says.
current=$HOME/.local/state/omarchy/current
# What `omarchy-theme-set` leaves behind: the theme's files in a directory of their own, and its name beside it.
stage_theme() {
  mkdir -p "$current/$2"
  cp -- "$1/colors.toml" "$current/$2/colors.toml"
}
if [[ -n $theme ]]; then
  stage_theme "$theme" theme
  basename -- "$(realpath -- "$theme")" > "$current/theme.name"
fi

# A session bus with no service directories: nothing can be dbus-activated on it.
cat > "$root/dbus.conf" <<EOF_DBUS
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:abstract=omui-preview-$BASHPID</listen>
  <keep_umask/>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF_DBUS
mapfile -t dbus_details < <(/usr/bin/dbus-daemon --config-file="$root/dbus.conf" --fork --print-address=1 --print-pid=1)
(( ${#dbus_details[@]} == 2 )) || { echo "preview: could not start the isolated session bus" >&2; exit 1; }
export DBUS_SESSION_BUS_ADDRESS=${dbus_details[0]}
dbus_pid=${dbus_details[1]}

printf '%s\n' \
  "hl.monitor({ output = \"\", mode = \"$size@60\", position = \"auto\", scale = 1 })" \
  'hl.config({' \
  '  general = { gaps_in = 0, gaps_out = 0, border_size = 0 },' \
  '  decoration = { rounding = 0, shadow = { enabled = false }, blur = { enabled = false } },' \
  '  animations = { enabled = false },' \
  '  misc = { disable_hyprland_logo = true, disable_splash_rendering = true },' \
  '})' \
  'hl.window_rule({ match = { class = ".*" }, monitor = "omui" })' > "$root/hyprland.lua"
unset HYPRLAND_INSTANCE_SIGNATURE
/usr/bin/setsid /usr/bin/env WAYLAND_DISPLAY="$parent_socket" /usr/bin/Hyprland --config "$root/hyprland.lua" > "$root/hyprland.log" 2>&1 &
hypr_pid=$!
signature=""
display=""
for _ in {1..120}; do
  kill -0 "$hypr_pid" 2>/dev/null || { echo "preview: nested Hyprland exited during startup" >&2; sed -n '1,80p' "$root/hyprland.log" >&2; exit 1; }
  for instance in "$XDG_RUNTIME_DIR"/hypr/*; do
    [[ -S $instance/.socket.sock && -f $instance/hyprland.lock ]] || continue
    candidate=$(sed -n '2p' "$instance/hyprland.lock")
    [[ -n $candidate && -S $XDG_RUNTIME_DIR/$candidate ]] || continue
    signature=$(basename "$instance")
    display=$candidate
    break 2
  done
  sleep 0.1
done
[[ -n $signature ]] || { echo "preview: nested Hyprland did not publish its sockets" >&2; exit 1; }
export HYPRLAND_INSTANCE_SIGNATURE=$signature WAYLAND_DISPLAY=$display

# The nested compositor's own output is a window on the desktop this runs from, as large as that desktop tiles it.
# The gallery goes on a headless output instead, whose size is the monitor rule's; the window rule sends it there.
hyprctl output create headless omui >/dev/null

GDK_BACKEND=wayland "$binary" > "$root/gallery.log" 2>&1 &
gallery_pid=$!
on_output() {
  local output
  output=$(hyprctl monitors -j | jq 'first(.[] | select(.name == "omui") | .id)') &&
    hyprctl clients -j | jq -e --argjson output "${output:-null}" 'any(.[]; .mapped and .monitor == $output)' >/dev/null
}
for _ in {1..100}; do
  on_output && break
  kill -0 "$gallery_pid" 2>/dev/null || break
  sleep 0.1
done
shoot() {
  sleep "$settle"
  kill -0 "$gallery_pid" 2>/dev/null || { echo "preview: the gallery exited" >&2; sed -n '1,40p' "$root/gallery.log" >&2; exit 1; }
  # A picture of an output the window is not on would be blank, and look like a result.
  on_output || { echo "preview: the gallery's window is not on the output being captured" >&2; hyprctl clients >&2; exit 1; }
  mkdir -p "$(dirname "$1")"
  # Hyprland's own notices (it says it was started without its wrapper) would cover the header.
  hyprctl dismissnotify >/dev/null
  /usr/bin/grim -o omui "$1"
  echo "preview: $1"
}
if [[ -n $keys ]]; then
  # Once the window is drawn and what OMUI_GALLERY_OPEN names is open. The nested compositor's keyboard is the
  # gallery's: it is the only window there.
  sleep 1.5
  for key in $keys; do
    if [[ $key == *+* ]]; then
      wtype -M "${key%%+*}" -k "${key#*+}" -m "${key%%+*}"
    else
      wtype -k "$key"
    fi
    sleep 0.3
  done
fi
shoot "$screenshot"
if [[ -n $next_theme ]]; then
  stage_theme "$next_theme" next
  [[ ! -d $current/theme ]] || mv -- "$current/theme" "$current/previous"
  mv -- "$current/next" "$current/theme"
  rm -rf -- "$current/previous"
  basename -- "$(realpath -- "$next_theme")" > "$current/theme.name"
  shoot "${screenshot%.png}-switched.png"
fi
[[ -z ${OMUI_PREVIEW_LOG:-} ]] || cp -- "$root/gallery.log" "$OMUI_PREVIEW_LOG"
[[ ! -s $root/gallery.log ]] || sed -n '1,20p' "$root/gallery.log" >&2
