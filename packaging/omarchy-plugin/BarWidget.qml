// One icon. It starts the PODSHL client, and when the client is not there it
// offers to install it, once asked and in the open.
//
// `omarchy plugin add` clones files and runs nothing: no install hook, no sudo,
// by design (manual, "Shell Plugins"). So the package cannot arrive with the
// plugin. It arrives with the first click instead:
//
//   * a click asks the client, in a login shell, to name itself. Not whether
//     the name is on the PATH: a name resolves to stale shims and broken
//     symlinks, and one of those made this icon do nothing at all. It asks at
//     the click and not at shell start, because an answer from startup goes
//     stale the moment somebody installs the client another way;
//   * it answers, the client starts. It does not, a panel under the icon says
//     so and shows the exact command it would run;
//   * "Install" opens Omarchy's own floating terminal on `install-client.sh`,
//     which ships with this plugin: it builds `podshl-bin` from the PKGBUILD
//     in `package/` with makepkg. It no longer asks the AUR first — that
//     package is not there and cannot be while registration is paused. The
//     terminal is not decoration: it ends in `sudo pacman`, and a password
//     prompt needs somewhere to be typed. When it succeeds, the client starts.
//
// Nothing else happens here. The plugin runs unsandboxed inside the shell and
// belongs to a product whose argument is bounded effect, so it reads no files,
// runs no diagnosis, makes no network call and keeps no state. Everything that
// touches the machine lives in the client, behind its own consent panel.
//
// `omarchy-shell shell summon podshl.diagnose` does what a click does, because
// the shell routes summon to any bar widget with open(), close() and opened.

import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

BarWidget {
  id: root
  moduleName: "podshl.diagnose"

  readonly property string client: "podshl-client"
  // The script beside this file, as a path. Refused rather than quoted if it
  // holds a quote: it is pasted into a single-quoted shell string below.
  readonly property string installer: {
    var url = String(Qt.resolvedUrl("install-client.sh"))
    var path = decodeURIComponent(url.replace(/^file:\/\//, ""))
    return /['"\\$`]/.test(path) ? "" : path
  }
  readonly property string installerShown: {
    var home = Quickshell.env("HOME")
    return home && installer.indexOf(home + "/") === 0
      ? "~" + installer.slice(home.length) : installer
  }

  property bool offerOpen: false
  // 0 is "Not now", 1 is "Install". Enter on an untouched panel installs,
  // because the panel only exists to ask that one question.
  property int choice: 1

  readonly property bool opened: offerOpen

  function open() {
    if (probe.running) return
    probe.running = true
  }

  function close() {
    offerOpen = false
  }

  function launch() {
    if (root.bar) root.bar.run("setsid uwsm-app -- " + root.client)
  }

  function install() {
    offerOpen = false
    if (!root.bar || !root.installer) return
    // Single-quoted as a whole for the terminal wrapper, so nothing inside may
    // contain a single quote. The subshell keeps `&` from backgrounding the
    // installation too — the same shape as omarchy-install-and-launch.
    //
    // The failure line is ours because the wrapper's closing prompt is not:
    // Omarchy 4.0.4 ends every run with "Done! Press any key", whatever the
    // exit code, and a failed install must not read as a finished one.
    root.bar.run("omarchy-launch-floating-terminal-with-presentation '"
      + "if bash \"" + root.installer + "\"; then "
      + "(setsid uwsm-app -- gtk-launch " + root.client + " >/dev/null 2>&1 &); "
      + "else echo; echo PODSHL was not installed. The lines above say why.; false; fi'")
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  Process {
    id: probe
    // A login shell, because that is the PATH bar.run() launches with.
    //
    // **It asks the client to name itself, not the shell to find the name.**
    // `command -v` answers about a *name*, and a name resolves to plenty of
    // things that are not a working client. On a real Omarchy machine on
    // 2026-09-18 it resolved to a dead `mise` shim left behind by an
    // uninstalled tool: `command -v` exited 0, so this launched instead of
    // offering to install, and `mise` then failed into a terminal nobody sees
    // — *and exited 0 while doing it*. The person clicked the icon and got
    // nothing at all: no window, no panel, no message. That is the failure
    // this plugin exists to avoid, not one it may cause.
    //
    // So the test is the one thing a working client can always do and a stale
    // symlink cannot: say its own name (`L8`). The exit code is not enough,
    // because `mise` returns 0 on its own error; the output has to match.
    command: ["bash", "-lc", root.client + " --version 2>/dev/null | grep -q '^" + root.client + " '"]
    onExited: function(exitCode) {
      if (exitCode === 0) {
        root.launch()
      } else {
        root.choice = 1
        root.offerOpen = true
      }
    }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    tooltipText: root.offerOpen ? "" : "PODSHL: diagnose a problem"

    // nf-md-bug. The solid one rather than nf-fa-bug, whose legs are a hairline
    // and disappear at the size a bar actually renders. Named explicitly rather
    // than inherited: the glyph is the whole widget, and a bar font without it
    // would leave a box where the icon should be.
    // Written as the character itself. A `\uXXXX` escape takes four hex
    // digits, so `4` is U+F00E followed by the digit 4 — a different
    // icon with a stray number beside it, which validates, loads, and looks
    // wrong in a way nobody reads twice.
    text: "󰃤"
    fontFamily: "JetBrainsMono Nerd Font"
    horizontalMargin: 7.5

    onPressed: function (which) {
      if (!root.bar) return
      if (which === Qt.RightButton)
        root.bar.run("xdg-open https://github.com/dx111ge/podshl")
      else if (root.offerOpen)
        root.close()
      else
        root.open()
    }
  }

  KeyboardPanel {
    id: offer
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.offerOpen
    focusTarget: keys
    contentWidth: offer.fittedContentWidth(Style.space(380))
    contentHeight: offer.fittedContentHeight(column.implicitHeight)

    PanelKeyCatcher {
      id: keys
      anchors.fill: parent
      onMoveRequested: function (dx, dy) {
        if (dx !== 0) root.choice = dx < 0 ? 0 : 1
      }
      onTabRequested: function (direction) {
        root.choice = root.choice === 0 ? 1 : 0
      }
      onActivateRequested: {
        if (root.choice === 1) root.install()
        else root.close()
      }
      onCloseRequested: root.close()

      Column {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Style.space(10)

        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: "PODSHL is not installed"
          color: Color.popups.text
          font.family: Style.font.family
          font.pixelSize: Style.font.title
          font.bold: true
        }

        Text {
          width: parent.width
          textFormat: Text.PlainText
          wrapMode: Text.WordWrap
          text: "This icon only starts the PODSHL client, and the client is not on this machine yet. "
            + "Install it? A terminal opens and runs the script below, which came with this plugin: "
            + "it builds the package podshl-bin from the PKGBUILD shipped beside it, checking the "
            + "download against the sums in that file. The package manager asks for your password, "
            + "and PODSHL starts when it is done."
          color: Color.popups.text
          font.family: Style.font.family
          font.pixelSize: Style.font.body
        }

        Text {
          width: parent.width
          textFormat: Text.PlainText
          wrapMode: Text.WrapAnywhere
          text: "$ bash " + root.installerShown
          color: Color.accent
          font.family: Style.font.family
          font.pixelSize: Style.font.body
        }

        Row {
          anchors.right: parent.right
          spacing: Style.spacing.controlGap

          Button {
            text: "Not now"
            fontFamily: Style.font.family
            hasCursor: root.choice === 0
            onHovered: function (isHovered) { if (isHovered) root.choice = 0 }
            onClicked: root.close()
          }

          Button {
            text: "Install"
            bordered: true
            fontFamily: Style.font.family
            hasCursor: root.choice === 1
            onHovered: function (isHovered) { if (isHovered) root.choice = 1 }
            onClicked: root.install()
          }
        }
      }
    }
  }
}
