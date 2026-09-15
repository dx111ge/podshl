# PODSHL for Omarchy

One bar icon. Click it to start the PODSHL client, which asks the projects you
actually run what is wrong with your machine — and asks you before it reads
anything. Right-click opens the project page.

## What this plugin does, and what it deliberately does not

It starts a program. That is all of it.

The marketplace says plainly that **plugins run unsandboxed**, and this one is
for a product whose entire argument is bounded effect. So it reads no files,
runs no diagnosis, makes no network call and keeps no state. Everything that
touches your machine lives in the client, behind a consent panel that shows
each reading before it is taken and an anonymiser that strips paths, accounts
and addresses before anything travels.

Reimplementing any of that here would be a second copy of the safety argument,
drifting from the first.

## Where to look, because the icon is small

It is **one monochrome bug glyph in a 26-pixel bar**, and on a wide screen that
is genuinely hard to spot. It lands in the **centre** section by default, which
on most setups puts it immediately to the right of the clock. Look there first.

That is a limit of what a bar widget is, not a bug: the bar is icons, and an
icon among icons is what this can be. If you want it somewhere you will actually
notice, move it:

    omarchy bar move podshl.diagnose --section right --index 0
    omarchy bar move podshl.diagnose --after omarchy.clock

Right-click opens the project page, which is the quickest way to confirm you
found the right icon.

If you would rather have a row you can read than an icon you have to find, the
menu entry below is the better fit — a label and a description, in the place you
already go when you want to do something.

## It needs the client

`omarchy plugin add` copies files and runs nothing, so this cannot install
anything for you. Install the client first:

    https://github.com/dx111ge/podshl

**If the icon does nothing, the client is not on your `PATH`.** That is the
whole failure mode, and it is worth stating because a bar icon that silently
does nothing is indistinguishable from a broken plugin.

## What the client is for

A project publishes three files on a host it controls: a challenge, a manifest
of what it needs to know, and its answers. Your machine reads only what that
project asked for, one consent at a time, and matches locally. Nothing is
generated, and nothing about your problem is inferred.

When nothing published covers your problem, you get the report you would have
had to assemble by hand — anonymised, with measured and supplied facts kept
apart — to paste into the project's issue tracker.

## Licence

AGPL-3.0-or-later, the same as the client.

## Why a bar icon and not a menu entry

`kind: "menu"` in Omarchy does not mean *contribute an entry*; it means *be the
menu* — an `Item` with `open()`, `close()`, `refresh()` and `ping()`, summoned
by name. Adding an entry to the Omarchy menu is a user config file
(`~/.config/omarchy/extensions/omarchy-menu.jsonc`) and not a plugin at all.

So a bar widget is what a plugin can honestly be here. A menu entry is a few
lines in that file, needs nothing from this repository, and is easier to find
than an icon:

```jsonc
"podshl": {
  "icon": "\udb80\udce4",
  "label": "Diagnose a problem",
  "description": "Ask the projects you run what is wrong — readings consented one at a time",
  "action": "podshl-client",
  "when": "command -v podshl-client"
}
```

`when` hides the row when the client is not installed, which is better than a
row that does nothing.
