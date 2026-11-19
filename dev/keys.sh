#!/usr/bin/env bash
set -Eeuo pipefail

# The keyboard, checked with real key presses: each case opens one of the gallery's dialogs or pages in the nested
# desktop of dev/preview.sh, presses keys, and compares what the gallery then reports (OMUI_GALLERY_TRACE: where the
# focus went, what was answered, and how a split or a stack of pages changed) with what is expected. "row" is the
# gallery's own sidebar row, which has the focus before a dialog opens and gets it back when the last one closes;
# "text" is an entry; a widget in a row is named with the row's title.
#
#   cargo build --example gallery && dev/keys.sh
here=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
scratch=$(mktemp -d /tmp/omui-keys.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT
failed=0

# check <what OMUI_GALLERY_OPEN opens, page:<a page> to start with the keyboard in that page, or start> <keys>
#       <the lines expected, joined by |>
# SIZE=<width>x<height> before it makes the window that size; TRACE=<kinds> compares those lines of the trace in place
# of the focus, answers, shortcuts, splits and pages.
check() {
  local got target=(OMUI_GALLERY_OPEN="$1")
  [[ $1 != page:* ]] || target=(OMUI_GALLERY_PAGE="${1#page:}")
  [[ $1 != start ]] || target=()
  env "${target[@]}" OMUI_PREVIEW_SIZE="${SIZE:-1280x820}" OMUI_PREVIEW_SETTLE=1 OMUI_GALLERY_TRACE=1 \
    OMUI_PREVIEW_KEYS="$2" OMUI_PREVIEW_LOG="$scratch/log" \
    "$here/preview.sh" "$scratch/shot.png" >/dev/null 2>"$scratch/errors" || { cat "$scratch/errors" >&2; exit 1; }
  # GTK reports no focus for an instant as Tab goes round from the last widget to the first.
  got=$(grep -E "^(${TRACE:-focus|answer|shortcut|split|nav}): " "$scratch/log" | grep -vx 'focus: nothing' | paste -sd'|')
  if [[ $got == "$3" ]]; then
    echo "ok    $1${SIZE:+ at $SIZE} [$2]"
  else
    echo "FAIL  $1${SIZE:+ at $SIZE} [$2]"
    echo "      expected: $3"
    echo "      got:      $got"
    failed=1
  fi
}

# Tab and Shift+Tab go round inside the dialog; Escape closes it, answers nothing, and the focus returns.
check confirm 'Tab Tab shift+ISO_Left_Tab shift+ISO_Left_Tab Escape' \
  'focus: row|focus: _Cancel|focus: Discard|focus: _Cancel|focus: Discard|focus: _Cancel|focus: row'
# Enter gives the default response: Cancel when the action is destructive, the action when it is not.
check confirm 'Return' 'focus: row|focus: _Cancel|focus: row'
# Alt+C cancels.
check confirm 'alt+c' 'focus: row|focus: _Cancel|focus: row'
check send 'Return' 'focus: row|focus: _Send|focus: row|answer: sent'
# A mnemonic is the dialog's own: Alt+D does not press "Discard a draft…" on the page under it, and Alt+S sends.
check send 'alt+d alt+s' 'focus: row|focus: _Send|focus: row|answer: sent'
# Enter in an entry that activates the default.
check rename 'Return' 'focus: row|focus: text|focus: row|answer: rename: Receipts'
# A disabled response is given neither by Enter nor by Tab, and Escape gives the close response.
check rename 'BackSpace Return Tab Tab Escape' \
  'focus: row|focus: text|focus: Cancel|focus: text|focus: row|answer: cancel: '
# The window's shortcuts and the application's accelerators do nothing while a dialog is open, and act again once it
# has closed: Delete would otherwise ask a second time, Ctrl+U and F5 act behind the question.
check start 'Delete Delete ctrl+u F5 Escape F5' \
  'focus: row|shortcut: delete|focus: _Cancel|focus: row|shortcut: refresh'
# A dialog that cannot be closed ignores Escape. A dialog over it takes the keyboard, and gives it back.
check upload 'Escape Tab Return Tab Return' \
  'focus: row|focus: Stop|focus: Stop|focus: _Cancel|focus: Stop|focus: Stop|focus: row'
# A response that leaves the dialog open: Clear answers, and Escape then closes it.
check log 'alt+l Escape' 'focus: row|focus: textview|answer: clear|focus: row|answer: close'
# In a dialog taller than the window the focus starts in the list that scrolls.
check tall 'Escape' 'focus: row|focus: checkbutton Calendar 1|focus: row|answer: done'
# A window without a shell gets a modal window, with the same keys. The gallery is tiled beside the editor window,
# too narrow for its sidebar, and the focus goes with the page.
check window 'Tab Return' \
  'focus: row|split: window collapsed sidebar|split: window collapsed content|focus: _Discard a draft…|answer: deleted'

# Omacloud's recovery code. Its second response waits for the code's end to be typed, and Tab passes it by until then.
check recovery 'Tab Tab w 3 h c Tab Tab Return' \
  'focus: row|focus: text Type its last four characters|focus: Copy|focus: text Type its last four characters|focus: Copy|focus: I Wrote It Down|focus: row|answer: done'
# Escape gives Copy, the close response, and the app shows the dialog again.
check recovery 'Escape Escape' \
  'focus: row|focus: text Type its last four characters|focus: row|answer: copy|focus: text Type its last four characters|focus: row|answer: copy|focus: text Type its last four characters'
# Contacts' edit form. Save's mnemonic does nothing while no name is left, and saves once there is one.
check contact 'Tab ctrl+a BackSpace Tab ctrl+a BackSpace alt+s x alt+s' \
  'focus: row|focus: button iCloud (ada@example.org)|focus: text First name|focus: text Last name|focus: row|answer: save:  x in iCloud (ada@example.org)'

# Rows. Tab goes from row to row: a row that carries a control is one stop, the control; a disabled row is none. Enter
# on an expander's row closes it, and Tab then passes its rows by; Enter on a closed one opens it.
check page:rows 'Tab Tab Tab Tab Tab Tab Tab Tab Tab Tab Tab Tab Tab Tab shift+ISO_Left_Tab Return Tab Return Tab' \
  'focus: row|focus: Add|focus: row Harbour office|focus: row Storage|focus: row A subtitle kept to two lines|focus: row Signed in as ada@example.org|focus: Sign Out|focus: text Name|focus: text Email|focus: text Password|focus: Show the text|focus: button Every hour|focus: switch Notifications|focus: switch Metered connections|focus: row harbour-laptop|focus: switch Documents|focus: row harbour-laptop|focus: row Lost every computer?|focus: text Recovery code'
# The arrows go from row to row as well: past a row's button, through the controls that rows carry, from one group to
# the next, into an open expander and out of it, and past a closed one's rows. At the last row Down stays there.
check page:rows 'Tab Down Down Down Down Down Down Down Down Down Down Down Down Down Down Down Down Up Up Up' \
  'focus: row|focus: Add|focus: row Harbour office|focus: row Storage|focus: row A subtitle kept to two lines|focus: row Signed in as ada@example.org|focus: text Name|focus: text Email|focus: text Password|focus: button Every hour|focus: switch Notifications|focus: switch Metered connections|focus: row harbour-laptop|focus: switch Documents|focus: switch Pictures|focus: row Lost every computer?|focus: row Account|focus: row Version|focus: row Account|focus: row Lost every computer?|focus: switch Pictures'
# In a dialog's form too.
check contact 'Down Down Up' \
  'focus: row|focus: button iCloud (ada@example.org)|focus: text First name|focus: text Last name|focus: text First name'
# Tabs. Enter on one shows its page, which is where Tab goes after the last of them.
check page:tabs 'Tab Return Tab Tab Tab' \
  'focus: row|focus: button Overview|focus: button Devices|focus: button Settings|focus: button About|focus: row harbour-laptop'

# Pages. Enter on a row pushes one and the focus goes into it; Escape and Alt+Left pop it, and the focus is back on
# the row.
check page:nav 'Return Escape Tab Return alt+Left' \
  'focus: row|focus: row Create an account|focus: Back|nav: create|focus: row Create an account|nav: root|focus: row Join an account|focus: Back|nav: join|focus: row Join an account|nav: root'
# A page pushed by an action leaves the keyboard on the sidebar, and Escape still goes back.
check push 'Escape' 'focus: row|nav: advanced|nav: root'
# Two deep, and back to the root in one step.
check page:nav 'Return Tab Tab Tab Tab Tab Tab Tab Return Tab Return' \
  'focus: row|focus: row Create an account|focus: Back|nav: create|focus: button Cloudflare R2|focus: text Cloudflare account ID|focus: text Access key ID|focus: text Secret access key|focus: Show the text|focus: row Lost every computer?|focus: row Advanced|focus: switch Sync settings too|nav: advanced|focus: Start over|focus: row Create an account|nav: root'

# The gallery's sidebar is one Tab stop, so Tab does not show each page it passes: on to its footer, then the page.
check start 'Tab Tab shift+ISO_Left_Tab shift+ISO_Left_Tab Down' \
  'focus: row|focus: button Connected|focus: Default|focus: button Connected|focus: row|focus: row'
# A split wide enough for both panes: Tab goes through the sidebar, its divider, and on into the content.
check page:split 'Tab Down Return Tab Tab Tab Tab Tab Tab Tab' \
  'focus: row|focus: text Search contacts|focus: row Ada Lovelace|focus: row Grace Hopper|split: contacts wide both|focus: row Katherine Johnson|focus: row Linus|focus: row Margaret Hamilton|focus: row No name|focus: switch Mirror contacts between iCloud and Google|focus: separator|focus: Edit'
# The divider: Right and Left move it a step, Home and End to the sidebar's least and most, and Right at the most
# stays there.
TRACE=divider check page:split 'Tab Tab Tab Tab Tab Tab Tab Tab Right Left Left Home End Right' \
  'divider: 296|divider: 280|divider: 264|divider: 240|divider: 360'
# The sidebar put away and brought back from its button in the header bar.
check start 'shift+ISO_Left_Tab shift+ISO_Left_Tab shift+ISO_Left_Tab shift+ISO_Left_Tab shift+ISO_Left_Tab Return Return' \
  'focus: row|focus: button|focus: New|focus: Search|focus: Back|focus: Sidebar|split: window wide content|split: window wide both'
# Too narrow for both. A list and its detail: Enter on a row shows the content and the focus goes into it; Escape and
# Alt+Left go back to the list, each time with the focus where it was.
SIZE=480x820 check page:split 'Tab Down Return Tab Tab Tab Escape Return alt+Left' \
  'focus: row|focus: text Search contacts|split: contacts collapsed sidebar|split: window collapsed sidebar|split: window collapsed content|focus: row Ada Lovelace|focus: row Grace Hopper|focus: Contacts|split: contacts collapsed content|focus: Edit|focus: Delete|focus: label Grace Hopper|focus: row Grace Hopper|split: contacts collapsed sidebar|focus: label Grace Hopper|split: contacts collapsed content|focus: row Grace Hopper|split: contacts collapsed sidebar'
# A sidebar that is put away: the window opens on it, and Enter on a row shows that row's page.
SIZE=480x820 check start 'Down Return' \
  'focus: row|split: window collapsed sidebar|focus: row|focus: flowboxchild plus|split: window collapsed content'
# A window as narrow as its tile keeps that width, with a switch whose title wraps in the split's sidebar.
TRACE=width SIZE=360x820 check page:split '' 'width: 360'
exit "$failed"
