<#
    Build the Windows installer, checksum it, and say what it is for.

    An NSIS setup that installs for the current user only — no administrator,
    no UAC prompt, nothing written outside the person's own profile. The
    client is an application that promises bounded effect; an installer that
    asked for the whole machine first would contradict it before it started.
    WebView2 is part of Windows 11 and of every supported Windows 10; where it
    is missing the installer fetches Microsoft's bootstrapper.

    A release is built for one operator. The installed client has nobody to set
    its environment, so the operator's address, the index and the log's public
    key are compiled in (`PODSHL_BUILD_*`); the environment still overrides them
    for development. With no -ServerUrl it is built for this machine's own
    stack, and the file name says so, so a loopback build is not mistaken for
    one anybody else can use.

        pwsh scripts\build\build_windows_installer.ps1
        pwsh scripts\build\build_windows_installer.ps1 -ServerUrl https://operator.example -IndexUrl https://operator.example/index -LogKey path\to\log_key.json

    Needs Rust, Node (for the Tauri CLI, fetched by npx) and a network the first
    time, when Tauri downloads NSIS. Not signed: see PLATFORM-windows.md.
#>
[CmdletBinding()]
param(
    [string]$ServerUrl = 'http://127.0.0.1:8725',
    [string]$IndexUrl  = 'http://127.0.0.1:8723',
    [string]$LogKey
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $LogKey) { $LogKey = Join-Path $repo 'var\log_key.json' }

# The key is checked before an hour of compiling rather than after: a build
# without a usable key installs a client that refuses every index it is served.
if (-not (Test-Path $LogKey)) { throw "no log key at $LogKey — it is the operator's log_key.json" }
$keyText = (Get-Content -Raw $LogKey).Trim()
try { $jwk = $keyText | ConvertFrom-Json } catch { throw "$LogKey is not JSON" }
if (-not $jwk.kty -or -not $jwk.x) { throw "$LogKey is not a public JWK (no kty/x)" }
if ($jwk.d) { throw "$LogKey holds a private key (d) — only the public half is compiled in" }

$loopback = $ServerUrl -match '^https?://(127\.0\.0\.1|localhost|\[::1\])(:|/|$)'
$version = (Select-String -Path (Join-Path $repo 'client-rs\Cargo.toml') -Pattern '^version = "(.+)"' |
            Select-Object -First 1).Matches[0].Groups[1].Value

$env:PODSHL_BUILD_SERVER_URL = $ServerUrl.TrimEnd('/')
$env:PODSHL_BUILD_INDEX_URL  = $IndexUrl.TrimEnd('/')
$env:PODSHL_BUILD_LOG_KEY    = $keyText

Push-Location (Join-Path $repo 'client-rs')
try {
    # The helper for changes that need administrator rights, built first and
    # put where the installer hook picks it up (windows/installer-hooks.nsh).
    cargo build --release --bin podshl-elevate
    if ($LASTEXITCODE -ne 0) { throw "building podshl-elevate failed ($LASTEXITCODE)" }
    $helperDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    Copy-Item (Join-Path $helperDir 'release\podshl-elevate.exe') 'windows\podshl-elevate.exe' -Force

    npx --yes '@tauri-apps/cli@2' build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
    Remove-Item Env:\PODSHL_BUILD_SERVER_URL, Env:\PODSHL_BUILD_INDEX_URL, Env:\PODSHL_BUILD_LOG_KEY -ErrorAction SilentlyContinue
}

# **Every file this script writes gets LF, explicitly.**
#
# `Set-Content` writes CRLF on Windows, and `SHA256SUMS` is read by
# `sha256sum -c` on Linux and macOS -- which takes the carriage return as part
# of the file name and reports every line as "No such file or directory". The
# checksum file is for the people who did not build it, and they are mostly not
# on Windows.
#
# 0.1.0 through 0.1.2 escaped this by accident: the Linux task happened to write
# the file last, with LF. So whether a release could be verified at all depended
# on the order two builds were run in, which is not a property to leave standing
# once it is known.
function Write-Lf([string]$Path, [string[]]$Lines) {
    [System.IO.File]::WriteAllText($Path, ($Lines -join "`n") + "`n", `
        (New-Object System.Text.UTF8Encoding $false))
}

# Honour CARGO_TARGET_DIR, as the Linux release task does.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'client-rs\target' }
$built = Get-ChildItem (Join-Path $target 'release\bundle\nsis') -Filter "*_${version}_*-setup.exe" |
         Sort-Object LastWriteTime | Select-Object -Last 1
if (-not $built) { throw "no setup for $version under $target\release\bundle\nsis" }

$out = Join-Path $repo "var\release\$version"
New-Item -ItemType Directory -Force $out | Out-Null
$name = if ($loopback) { "podshl-client-$version-windows-x64-loopback-setup.exe" } else { "podshl-client-$version-windows-x64-setup.exe" }
Copy-Item $built.FullName (Join-Path $out $name) -Force

$hash = (Get-FileHash -Algorithm SHA256 (Join-Path $out $name)).Hash.ToLower()

# Which commit this came from, the way the Linux task records it. Without it
# the two PLATFORM files in one release directory answer "what was built?"
# differently -- one names a commit, the other only a version, and a version is
# not something anybody can check out.
$commit = (git -C $repo rev-parse --short HEAD 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $commit) { $commit = 'unknown' }
$dirty = (git -C $repo status --porcelain 2>$null)
if ($dirty) { $commit = "$commit+dirty" }

$for = if ($loopback) {
    "this machine's own stack at $ServerUrl. It is useless anywhere else: another computer has no operator on its loopback address. Build again with -ServerUrl for anything you hand out."
} else {
    "the operator at $ServerUrl, index $IndexUrl."
}
Write-Lf (Join-Path $out 'PLATFORM-windows.md') @"
# podshl-client — Windows, x64

## What this is

``$name``, an installer for the customer-side agent. It verifies a vendor,
collects locally under per-item consent, and returns a conclusion. It never
signs anything, and it can only perform operations it already implements.

Built for $for The operator's log key is compiled in; ``PODSHL_SERVER_URL``,
``PODSHL_INDEX_URL`` and ``VS_LOG_KEY`` still override it.

## What it does to the machine

Installs for the current user only, under ``%LOCALAPPDATA%\PODSHL``, with a
Start menu entry and an entry in Installed apps. Installing needs no
administrator and shows no UAC prompt. State lives in ``%APPDATA%\podshl``. Uninstalling removes the program;
ticking "delete application data" also removes ``%APPDATA%\podshl``. An API key
stays in Windows Credential Manager (``de.podshl.client``) until it is removed
there.

WebView2 ships with Windows 11 and supported Windows 10. Where it is missing,
the installer downloads Microsoft's bootstrapper, which needs a network.

## What is not here

A signature. Windows SmartScreen will say it does not recognise the publisher,
and that is true: a signature says who built it, and there is no certificate
yet. The SHA256SUMS beside this file says what was built, not by whom.

## Changes that need administrator rights

``podshl-elevate.exe`` is installed next to the client. It performs three
example actions and nothing else — restart a service, set how a service
starts, set or remove a machine-wide environment variable — and only when the
client starts it through Windows' own administrator prompt, which the person
answers for each change. Services and variables Windows depends on are refused.
The client reads the result back itself and records every change, with its
undo, in ``%APPDATA%\podshl\repairs.json``.

## Provenance

podshl-client $version, built from $commit, SHA-256 $hash.
"@

# **The sums are written last, after every file they cover.** They used to be
# written before `PLATFORM-windows.md`, so the one file that tells a reader
# "the SHA256SUMS beside this file says what was built" was the one file the
# sums did not mention. Found by cutting 0.1.3 and reading the directory.
$sums = Join-Path $out 'SHA256SUMS'
$keep = if (Test-Path $sums) {
    Get-Content $sums | Where-Object { $_ -notmatch '\s(PLATFORM-windows\.md|' + [regex]::Escape($name) + ')$' }
} else { @() }
$rows = @($keep) + "$hash  $name"
$phash = (Get-FileHash -Algorithm SHA256 (Join-Path $out 'PLATFORM-windows.md')).Hash.ToLower()
$rows += "$phash  PLATFORM-windows.md"
Write-Lf $sums $rows

Write-Output "$out\$name"
Write-Output "sha256 $hash"
