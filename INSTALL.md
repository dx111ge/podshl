# Installing the client

The client is the part that runs on **your** machine. It reads what you allow it
to read, shows you a fix before applying it, and can undo what it did.

Nothing about your machine is uploaded until you have seen it and agreed. Every
reading is shown to you first; when the answer has to come from somewhere else
— the project's operator, a vendor's own endpoint, or the model you chose — the
client names the recipient, shows the values, and sends only after you say so.

## Download

**[github.com/dx111ge/podshl/releases/latest](https://github.com/dx111ge/podshl/releases/latest)**
— built for the beta operator at **https://sdota.de**.

| Platform | File | State |
|---|---|---|
| **Windows** 10/11 x64 | `podshl-client-0.1.5-windows-x64-setup.exe` | Installed and checked against the live operator |
| **Linux** x86_64 | `PODSHL_0.1.5_amd64.deb`, or the bare `podshl-client-0.1.5-linux-x86_64` | Built and its suite run in a container; the packaged window has not been walked on a Linux desktop |
| **macOS** Apple Silicon | `podshl-client-0.1.5-macos-arm64.dmg` | Built on GitHub's macOS runners, **never run by us** — a report of how it went is very welcome |

**None of them is signed.** A signature says who built something, and there is
no certificate yet. `SHA256SUMS` (and `SHA256SUMS-macos`) in the release say the
files arrived intact, not who made them.

## Windows

Run the setup. It installs for **your user only** — no administrator, no UAC
prompt — under `%LOCALAPPDATA%\PODSHL`, with a Start menu entry. Windows
SmartScreen will say "Windows protected your PC", because the file is unsigned:
**More info → Run anyway**. WebView2, which the window needs, ships with
Windows 11 and supported Windows 10; where it is missing, the setup downloads it.

## macOS

Open the `.dmg` and drag PODSHL to Applications. Because it is unsigned, macOS
refuses it as "damaged" or from "an unidentified developer" until the download
quarantine is removed — once, in Terminal:

```bash
xattr -dr com.apple.quarantine /Applications/PODSHL.app
```

## Linux

```bash
sudo apt install ./PODSHL_0.1.5_amd64.deb
podshl-client
```

The package declares what it needs rather than bundling it —
`libwebkit2gtk-4.1-0, libgtk-3-0` — because hiding 140-odd system libraries in a
hundred-megabyte download would cost what makes a binary of under 7 MB
auditable. Or run the bare binary with those installed:

```bash
chmod +x podshl-client-0.1.5-linux-x86_64
./podshl-client-0.1.5-linux-x86_64
```

**On Wayland the client turns WebKitGTK's DMA-BUF renderer off for itself**, by
setting `WEBKIT_DISABLE_DMABUF_RENDERER=1` in its own process before the window
is created. You do not have to do anything, and nothing outside this program is
affected. Without it the client does not merely draw a black window — it exits
before any window exists, with one line on standard error that a desktop icon
throws away, so clicking it appears to do nothing at all.

If you would rather have the accelerated path, set the variable yourself to any
value and the client leaves your choice alone:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=0 podshl-client
```

If the window then fails to appear, that is the failure this default exists for.

### What it needs on the machine

The client asks the operating system's credential store for an API key, so on
Linux that is a Secret Service provider — GNOME Keyring or KWallet — and
without one the model settings cannot store a key. Readings that name a program
need that program: `docker` for anything about a running container,
`nvidia-smi` for the graphics card, `lspci` for the PCI list. `doctor` says
which of these are missing here and what that leaves unreadable.

## Arch, and Omarchy

Omarchy is the desktop this client is aimed at first, and the one the workaround
above was measured on. On any Arch system the client is the pacman package
`podshl-bin` — the released binary, with its desktop entry and icons. It is
**not in the AUR yet** (registration there is paused); its PKGBUILD is in this
repository and builds as it is:

```bash
cp -r packaging/aur/podshl-bin /tmp/podshl-bin
cd /tmp/podshl-bin && makepkg -si
```

**On Omarchy, add the plugin instead** and let it do that for you. Its bar icon
offers the installation on the first click, in Omarchy's own terminal, and
starts PODSHL afterwards:

```bash
omarchy plugin add https://github.com/dx111ge/omarchy-podshl --enable
```

From 0.1.5 on, the client needs no model setup there either: it uses the
desktop's default agent (`~/.config/omarchy/defaults/agent`) until you choose another model. See
`omarchy-plugin/README.md`.

Without a package manager, the bare binary from the release works too, with
`webkit2gtk-4.1` and `gtk3` installed:

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3
chmod +x podshl-client-0.1.5-linux-x86_64
./podshl-client-0.1.5-linux-x86_64
```

**If you are running a client older than this one and nothing happens when you
start it**, that is the bug above and not a broken download. Start it from a
terminal to see the line the icon discards, and then:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./podshl-client-0.1.5-linux-x86_64
```

Measured 2026-09-14 on Omarchy 4.0.2 — Hyprland 0.56.2, webkit2gtk 2.52.6,
NVIDIA 610.57.04 — where the client exits with `Gdk-Message: Error 71 (Protocol
error) dispatching to Wayland display.` unless the renderer is off. Note the
driver version: support content in the wild, including our own worked example in
`spec/example-desktop/`, says this stopped happening after NVIDIA 555. On this
machine it did not, which is why the client no longer decides it by version.

## Which operator it talks to

A release is built for one operator: **https://sdota.de**, with the public key of
its transparency log compiled in, so the client refuses an index or a log head
that key did not sign. Compare it with `curl -s https://sdota.de/log/key` and
`release/sdota.de/log_key.json` in the repository. `PODSHL_SERVER_URL`,
`PODSHL_INDEX_URL` and `VS_LOG_KEY` point a client somewhere else — a local
development stack, or an operator of your own. The operator is new: until
projects publish their files there, the client finds no published answers and
says so rather than guessing.
## First, ask what it can do here

```
$ podshl-client doctor

PODSHL — what this client can do on this machine

  Platform      linux / x86_64
  Readable      9 values
                · gpu.name
                · os.version
                ...
  Not readable  5 — and why:
                · pci.devices  lspci is not available on this device — this reading cannot be taken here.
  Actions       3
                · report_only
                · restore_backup  (changes things)
                · set_config_key  (changes things)

  Limits        at most 24 readings per diagnosis
```

This is the honest inventory: what it can read on *this* machine, what it
cannot and why, and every action it is capable of. Nothing outside that list can
be asked of it by anyone.

`podshl-client demo` walks the whole argument in five acts against live services.

## Do I need to set up an AI model?

**No.** For any project that has published support files, everything is matched
against rules on the operator's endpoint, using the readings you agreed to send.
No model is involved.

A model is used in exactly one case: **nobody has published anything about your
problem**, and there is then no vendor to own what a model would say — so it
falls to one you chose. If you want that fallback, set it up under the chip in
the top right:

| | |
|---|---|
| **Free cloud tier** | GitHub Models, Google AI Studio, Cerebras, Groq, Mistral, OpenRouter's `:free` models. A free key, no card |
| **Local** | Ollama, LM Studio, llama.cpp. The only option where the question genuinely stays on your machine |
| **Paid cloud** | Anthropic, OpenAI, DeepSeek, Together |

**A cloud model means your question and your readings do leave the device** —
to the provider you picked, not to us. The settings screen says so rather than
letting "your own model" imply "stays local". Your API key goes into the
operating system's credential store, never into a config file, and it is only
ever sent to the host its provider preset names. The provider's address and
model name are kept in `llm.json` under your config directory, in plain text.

## What leaves your machine, and when

Nothing, unless you press a button that says so.

* **Readings are collected on your machine** and shown to you. To get an answer
  they then travel to whoever holds the rules — and you are asked first, with
  the recipient named and the values on the screen. For a project that
  published files that is the operator: it uses the readings, as read, to walk
  the project's rules, and does not keep them. For a vendor with its own agent
  it is the vendor's endpoint. Where nobody published anything, and only then,
  they go to the model you configured — which for a cloud model means the
  provider you picked.
* **Every read is a separate consent**, in plain language, with the evidence one
  disclosure behind. Read, transmit and change are three different questions.
  The **refusing** button holds focus, so a stray Return can never grant.
* **What you withhold becomes a question**, not a dead end.
* **Every change is dry-run first** and can be rolled back. The client can only
  perform operations it already implements — a project names one and fills in
  declared parameters; it cannot ship new capability.
* **A program may be asked its version** — only if the project asked, only after
  you agreed to that reading, and only the version number is kept. If the program
  is not on your search path you are asked where it is; your disk is never
  searched. System programs, shells and launchers are never started for this.
* **Reporting is optional and comes last**, after you have your answer and have
  said whether it worked. If you report, what travels is: the project's domain;
  the readings you consented to, coarsened first — a version reduced to
  major.minor, a memory size to a bucket, a serial never; the answers you gave
  from a list, and the name of a question you were asked but not the text you
  typed; whether it worked; which facts the answer turned on; the names of the
  readings you withheld; any action that failed; what resolved it and the
  version of the solution; the size class of your model, where one was used at
  all; a pseudonym; the month. There is no timestamp finer than the month, and
  the consent record for free text carries the month too. Every string in it
  passes the anonymiser before it leaves, on every path. Free text, including
  the lines of a log, travels only if you agree to that separately: you see the
  exact words first, with names, addresses, tokens and times already replaced,
  and you can edit every one of them.

### The pseudonym

Reports carry a pseudonym so a recipient can count people rather than posts, and
rate-limit abuse. It is **different for every project** and **changes every
month**, derived from 32 bytes of local randomness that is never a hardware
fingerprint. Across projects nothing links; across months nothing links. You can
reset it at any time — *Reset reporting identity*, in the settings — that forfeits
accumulated standing with every vendor, which is reset along with it, and
restores unlinkability. It is your call, not ours.

Nothing is shown to a maintainer until **five distinct people** have reported the
same thing. Below that a constellation is identifying. Five is the operator's
setting (`PODSHL_K`), not something a client or a maintainer can lower.

## Where it keeps things

Under your config directory — `%APPDATA%\podshl` on Windows,
`~/Library/Application Support/podshl` on macOS, `~/.config/podshl/` on Linux:

| | |
|---|---|
| `client_secret` | The 32 bytes the pseudonym is derived from. Deleting it is the reset |
| `llm.json` | Which model provider and model you chose, in plain text. The key itself is in the credential store |
| `index_cache.json` | The last index fetched, so the next start can ask whether it changed rather than fetch it again |
| `vendor_standing.json` | Your standing with each vendor, per pseudonym, reset with it |

A change the client applies leaves a `.bak` beside the file it changed, and it
never overwrites one that is already there.

## Uninstalling

| | |
|---|---|
| **Windows** | *Settings → Apps → Installed apps → PODSHL → Uninstall*. Ticking "delete application data" also removes `%APPDATA%\podshl` |
| **macOS** | Move `/Applications/PODSHL.app` to the Bin |
| **Linux** | `sudo apt remove podshl` |

Except where Windows' box is ticked, that leaves your data where it was: the
files above, the API key in the credential store, and any `.bak` beside a file
the client changed. Remove those yourself to leave nothing behind.
