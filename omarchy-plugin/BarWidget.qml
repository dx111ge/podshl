// One icon, and everything it refuses to do.
//
// The marketplace says plainly that plugins run unsandboxed, and this one
// belongs to a product whose entire argument is bounded effect. So it reads no
// files, runs no diagnosis, makes no network call and keeps no state. It starts
// a program the person installed; that program asks before it reads anything,
// shows what it found before anything travels, and can act only through three
// named actions under a directory the person granted.
//
// Putting any of that here would be a second copy of the safety argument,
// drifting from the first — and it would be the copy running without a sandbox.
//
// `omarchy plugin add` copies files and executes nothing, so this cannot
// install the client either. That is stated in the README rather than worked
// around: a widget that quietly installed software would be exactly the kind of
// program this one exists to be an alternative to.

import QtQuick
import qs.Ui

BarWidget {
  id: root
  moduleName: "podshl.diagnose"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar

    // nf-md-bug. The solid one rather than nf-fa-bug, whose legs are a hairline
    // and disappear at the size a bar actually renders. Named explicitly rather
    // than inherited: the glyph is the whole widget, and a bar font without it
    // would leave a box where the icon should be.
    // Written as the character itself. A `\uXXXX` escape takes four hex
    // digits, so `\uf00e4` is U+F00E followed by the digit 4 — a different
    // icon with a stray number beside it, which validates, loads, and looks
    // wrong in a way nobody reads twice.
    text: "󰃤"
    fontFamily: "JetBrainsMono Nerd Font"
    horizontalMargin: 7.5

    onPressed: function (which) {
      if (!root.bar) return
      if (which === Qt.RightButton)
        root.bar.run("xdg-open https://github.com/dx111ge/podshl")
      else
        root.bar.run("podshl-client")
    }
  }
}
