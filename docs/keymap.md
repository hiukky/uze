# Keys

uze is meant to be used without learning anything: everything on screen is
clickable, every action is offered where the thing it acts on is, and one
surface lists the rest. Keys are the accelerator on top of that — for the
fortieth session, not the first.

This page is for changing them. The Keys screen (`uze`, then the **Keys**
route) does everything below with a pointer; the file is here because a
keyboard is worth keeping in version control, and because a screen cannot
explain what a terminal will do with `Ctrl+1`.

## Where it lives

`~/.uze/keys.json`, beside `theme-overrides.json` and for the same reason:
it is something you wrote, never something uze can rebuild. It is
machine-scoped — a project does not get to decide what your fingers do.

It holds **only what differs** from what uze ships with. That is what lets
a later release move a key nobody had an opinion about instead of freezing
your keyboard at the version you first opened the screen on.

```json
{
  "bindings": {
    "workspace": {
      "new-shell-tab": ["f4"],
      "close-tab": []
    }
  }
}
```

Three things are being said there: `new-shell-tab` is now `F4` in the
workspace, `close-tab` has no key at all (the empty list is how you say
that — the action is still on its tab's close control and in the index),
and everything else is whatever uze ships with.

## The three parts

**An action** is a meaning: `remove-plugin`, `next-space`,
`toggle-git-changes`. Actions are the product's vocabulary; the Keys
screen lists every one, and the index (`F1`) lists the ones reachable
right now. An action is never a key — which is the whole point.

**A scope** is where an action is live: `global`, `plugins`, `workspace`,
`git-changes`, `preserved-work`. The innermost open surface answers first,
and a surface that seals — a dialog, a picker, the changes overlay —
answers for everything except `global`. That is what keeps *leave* and
*help* reachable from inside anything.

**A chord** is what you press: `ctrl+o`, `alt+shift+i`, `f2`,
`ctrl+pageup`, `/`. Modifiers in any order and any case, joined with `+`;
`alt+I` and `alt+shift+i` are the same chord written two ways.

## What a terminal can actually send

This is the part worth reading before you rebind anything, because a key
that never arrives looks exactly like a broken feature.

A terminal without an enhanced keyboard protocol has three ways to encode
a keystroke: a character, a control byte (`Ctrl` plus a letter), and an
escape sequence (function and navigation keys, and `Alt` as a prefix).
Everything else has no encoding at all.

**Refused outright.** Five control chords *are* other keys on a terminal,
and uze will not let you bind them:

| You wrote | It is |
|---|---|
| `ctrl+i` | Tab |
| `ctrl+m` | Enter |
| `ctrl+j` | line feed |
| `ctrl+h` | Backspace |
| `ctrl+[` | Esc |
| `ctrl+space` | NUL |

**Universal.** `Ctrl`+letter (except the above), `F1`–`F12`, `Shift+F…`,
and unmodified `Enter`/`Esc`/`Tab`/`Shift+Tab`/arrows/`Page…`/`Home`/`End`.
These reach uze over ssh, inside tmux, on WSL, anywhere.

**Depends on your terminal.** `Alt`+letter and `Alt`+digit are ESC-prefixed
and need the host to send Meta rather than compose a character: that is the
default on Linux, WSL and Windows Terminal, and **off by default on macOS
Terminal.app and iTerm2**, where Option composes accented characters. Also
here: modified navigation keys (`ctrl+up`, `ctrl+pageup`), which an
emulator often claims first for its own tabs.

**Needs an enhanced protocol.** `Ctrl`+digit, `Ctrl+Shift`+letter,
`shift+enter`, `ctrl+enter`. uze asks for the protocol at startup and uses
it when the terminal agrees (kitty, foot, WezTerm, ghostty, Alacritty,
recent iTerm2 and Windows Terminal). Where it does not, the Keys screen
says the chord cannot be sent rather than binding it and leaving it dead.

**On WSL specifically:** a Linux uze under Windows Terminal reads VT bytes
from a pty — the Win32 console key API is not in the path — so it sees
exactly what any Linux terminal sees. Every universal chord works, `Alt`
works, `Ctrl`+digit does not unless the protocol is on. Separately,
Windows Terminal keeps `Ctrl+Shift+*`, `Alt+Enter`, `Ctrl+Tab` and
`Ctrl+V` for itself, and under tmux or screen the multiplexer's own prefix
wins first. None of those ever reach uze.

**And the answer that is actually true for your machine:** the Keys
screen's capture *is* a probe. Press the key you are wondering about; it
says what arrived. No table can enumerate every emulator × multiplexer ×
ssh × layout, and this does not have to.

## The rules the screen enforces

- **A key means one thing per keyboard.** uze has two: management, where
  it owns the whole keyboard, and the workspace, where every bare key
  belongs to the agent running in the pane. Within one of them, two
  actions may not share a mnemonic — a letter, a digit, a function key.
  Structural keys (Enter, Esc, Tab, the arrows) are contextual by nature
  and are exempt: "go on" reads from the surface, and nobody is confused
  by it.
- **A conflict is refused before it is written**, and the screen says what
  the key already means.
- **A file that cannot be used changes nothing.** The keyboard already in
  force stays in force, and the problem is printed with the entry that
  caused it — because an operator without a keyboard cannot fix the file
  that took it away. An entry naming an action or a surface this build has
  never heard of is a *warning*: a keymap written for a newer uze still
  loads on an older one.

## What a keymap deliberately cannot change

- **What happens inside a pane.** Keys uze does not claim are the agent's,
  and rebinding one takes it away from every agent you run.
- **Mouse gestures.** Every action bound outside a pane is reachable with
  the pointer, and that is a property of the product rather than a
  preference — see `tests/architecture/affordances.rs`, which fails the
  build if an action stops saying where its pointer lands.
- **Chord sequences and modal states.** One chord, one action. uze is not
  vim and not tmux, and a prefix key is a course you have to take.
