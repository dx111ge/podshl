<#
    Build the client the way a release is built, and refuse to hand back one
    that is not. The Windows half of `scripts/build_client.sh`, with the same
    checks and the same refusals.

    `option_env!` is read at compile time and cargo does not rebuild when only
    an environment variable changed. So a client built once with the operator
    and the log key compiled in, and then rebuilt for any other reason without
    them, comes out silently pointing at loopback with no key — not a broken
    client, a *plausible* one: it starts, it draws its window, and it quietly
    cannot verify the published directory, so every project falls through to
    the model.

    That cost an afternoon on 2026-09-14 and it happened again on 2026-09-15,
    on this machine, to somebody who knew about it — twice, because the build
    here was `cargo build` typed by hand and nothing checked the result.

        pwsh scripts\build_client.ps1 sdota.de
        pwsh scripts\build_client.ps1 sdota.de -Profile debug

    For the installer, which is a different artefact, see
    `scripts\build_windows_installer.ps1`.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)]
    [string]$Operator,
    [ValidateSet('release', 'debug')]
    [string]$Profile = 'release',
    # The address to compile in, when it is not `https://<operator>`.
    #
    # On this LAN the router answers `sdota.de` with 91.12.73.134, which no
    # authoritative server claims and which nothing listens on; `www.sdota.de`
    # is a CNAME to the same name and is answered correctly by the same router.
    # So a client built here for the apex cannot reach an operator that is
    # perfectly healthy, and every project falls through to the model. The key
    # is still the operator's own — this changes the address, not the trust.
    #
    # Releases keep the apex: the authoritative record is right, and it is only
    # this network that is wrong.
    [string]$ServerUrl,
    # Where the built client is put so that everything which runs
    # `podshl-client` runs *this* one. A client that was built and not installed
    # is the same gap one step later.
    [string]$InstallDir = (Join-Path $env:USERPROFILE '.local\bin')
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

$keyFile = Join-Path $repo "release\$Operator\log_key.json"
if (-not (Test-Path $keyFile)) { throw "no public log key at $keyFile" }
$keyText = (Get-Content -Raw $keyFile).Trim()
$jwk = $keyText | ConvertFrom-Json
if (-not $jwk.kty -or -not $jwk.x) { throw "$keyFile is not a public JWK (no kty/x)" }
# Only ever the public half. Whoever holds the private half can sign a second,
# forged log that every client accepts.
if ($jwk.d) { throw "$keyFile holds a private key (d) — only the public half is compiled in" }

$base = if ($ServerUrl) { $ServerUrl.TrimEnd('/') } else { "https://$Operator" }
Write-Host "- building for $base ($Profile), key from $keyFile"

# `touch` so cargo recompiles the crate root: nothing else here changed, and
# without it the previous binary — the one without these values — is handed
# back as up to date.
(Get-Item (Join-Path $repo 'client-rs\src\main.rs')).LastWriteTime = Get-Date

$env:PODSHL_BUILD_SERVER_URL = $base
$env:PODSHL_BUILD_INDEX_URL  = $base
$env:PODSHL_BUILD_LOG_KEY    = $keyText
Push-Location (Join-Path $repo 'client-rs')
try {
    if ($Profile -eq 'release') { cargo build --release } else { cargo build }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
    Remove-Item Env:\PODSHL_BUILD_SERVER_URL, Env:\PODSHL_BUILD_INDEX_URL, Env:\PODSHL_BUILD_LOG_KEY -ErrorAction SilentlyContinue
}

# `CARGO_TARGET_DIR` moves the output. Looking in `client-rs\target` regardless
# is how the shell version once checked a file the build had not written.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'client-rs\target' }
$bin = Join-Path $target "$Profile\podshl-client.exe"
if (-not (Test-Path $bin)) { throw "no binary at $bin after building — wrong target directory?" }

# The path is named in every message below. A check that says what it found
# without saying where it looked is the shape of every wasted hour here: right
# about the file it read, and reading the wrong file.
Write-Host "- checking what actually landed in $bin"
$bytes = [System.IO.File]::ReadAllBytes($bin)
$ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
if (-not $ascii.Contains($base)) {
    throw "$base is not in $bin — it would talk to loopback"
}
if (-not $ascii.Contains($jwk.x)) {
    throw "the log key is not in $bin — it could not verify the directory, and every project would fall through to the model"
}

Write-Host '- asking the binary itself'
$ep = & $bin invoke endpoints '{}' | ConvertFrom-Json
Write-Host "  operator: $($ep.operator)"
if ($ep.operator -ne $base) { throw "the binary reports $($ep.operator), not $base" }

# Not `strings` and not the environment: ask the operator, with the key that was
# compiled in, for the thing the key exists to check. This is the only check
# here that can fail for the reason that actually matters, and the message it
# prints on failure now names the key it used.
Write-Host '- verifying the compiled key against the operator'
$refresh = & $bin invoke refresh_index (@{ base = $base } | ConvertTo-Json -Compress)
if ($LASTEXITCODE -ne 0 -or $refresh -notmatch '"entries"') {
    throw "the directory did not verify with the compiled key: $refresh"
}
Write-Host "  $refresh"

New-Item -ItemType Directory -Force $InstallDir | Out-Null
$dest = Join-Path $InstallDir 'podshl-client.exe'
Copy-Item $bin $dest -Force
$installedOp = (& $dest invoke endpoints '{}' | ConvertFrom-Json).operator
if ($installedOp -ne $base) { throw "installed client reports $installedOp, not $base" }

Write-Host "built and installed $dest for $base"
if (($env:PATH -split ';') -notcontains $InstallDir) {
    Write-Host "note: $InstallDir is not on PATH, so `podshl-client` still resolves elsewhere (or nowhere)"
}
