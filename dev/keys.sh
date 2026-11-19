#!/usr/bin/env bash
set -Eeuo pipefail

# The keyboard in a dialog, checked with real key presses: each case opens one of the gallery's dialogs in the nested
# desktop of dev/preview.sh, presses keys, and compares what the gallery then reports (OMUI_GALLERY_TRACE: where the
# focus went and what was answered) with what is expected. "row" is the sidebar row that has the focus before a dialog
# opens and gets it back when the last one closes; "text" is an entry.
#
#   cargo build --example gallery && dev/keys.sh
here=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
scratch=$(mktemp -d /tmp/omui-keys.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT
failed=0

# check <what OMUI_GALLERY_OPEN opens> <keys> <the focus and answer lines expected, joined by |>
check() {
  local got
  OMUI_PREVIEW_SETTLE=1 OMUI_GALLERY_OPEN=$1 OMUI_GALLERY_TRACE=1 OMUI_PREVIEW_KEYS=$2 OMUI_PREVIEW_LOG=$scratch/log \
    "$here/preview.sh" "$scratch/shot.png" >/dev/null 2>"$scratch/errors" || { cat "$scratch/errors" >&2; exit 1; }
  # GTK reports no focus for an instant as Tab goes round from the last widget to the first.
  got=$(grep -E '^(focus|answer): ' "$scratch/log" | grep -vx 'focus: nothing' | paste -sd'|')
  if [[ $got == "$3" ]]; then
    echo "ok    $1 [$2]"
  else
    echo "FAIL  $1 [$2]"
    echo "      expected: $3"
    echo "      got:      $got"
    failed=1
  fi
}

# Tab and Shift+Tab go round inside the dialog; Escape closes it, answers nothing, and the focus returns.
check confirm 'Tab Tab shift+Tab shift+Tab Escape' \
  'focus: row|focus: Cancel|focus: Discard|focus: Cancel|focus: Discard|focus: Cancel|focus: row'
# Enter gives the default response: Cancel when the action is destructive, the action when it is not.
check confirm 'Return' 'focus: row|focus: Cancel|focus: row'
check send 'Return' 'focus: row|focus: Send|focus: row|answer: sent'
# Enter in an entry that activates the default.
check rename 'Return' 'focus: row|focus: text|focus: row|answer: rename: Receipts'
# A disabled response is given neither by Enter nor by Tab, and Escape gives the close response.
check rename 'BackSpace Return Tab Tab Escape' \
  'focus: row|focus: text|focus: Cancel|focus: text|focus: row|answer: cancel: '
# A dialog that cannot be closed ignores Escape. A dialog over it takes the keyboard, and gives it back.
check upload 'Escape Tab Return Tab Return' \
  'focus: row|focus: Stop|focus: Stop|focus: Cancel|focus: Stop|focus: Stop|focus: row'
# A window without a shell gets a modal window, with the same keys.
check window 'Tab Return' 'focus: row|answer: deleted'
exit "$failed"
