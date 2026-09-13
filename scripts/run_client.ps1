<#
    Start the client on Windows with its window in front, and its console out
    of the way.

    A debug build is a console subsystem binary — `main.rs` asks for
    `windows_subsystem = "windows"` only under `not(debug_assertions)`, on
    purpose, because stdout is worth having while developing. The cost is a
    console window that opens in front of the application, so the thing you
    wanted to look at is behind the thing you did not.

    This starts the console minimised and raises the application window. It
    changes nothing about the build: a release binary has no console and the
    same script starts it just the same.

        pwsh scripts\run_client.ps1                 # debug, the default
        pwsh scripts\run_client.ps1 -Release
        pwsh scripts\run_client.ps1 -DebugPort 9333 # for scripts\drive_window.mjs
#>
[CmdletBinding()]
param(
    [switch]$Release,
    # Opens the WebView2 DevTools protocol on this port. Off unless asked for:
    # it is a debugging door into the window and does not belong on by default.
    [int]$DebugPort = 0,
    # Where the client keeps its state for this run. The default is the
    # repository's own `var/`, which is what every other task here uses.
    [string]$Root,
    [string]$ServerUrl = 'http://127.0.0.1:8725',
    [string]$IndexUrl  = 'http://127.0.0.1:8723'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Root) { $Root = Join-Path $repo 'var' }

$exe = Join-Path $repo ("client-rs\target\{0}\podshl-client.exe" -f ($Release ? 'release' : 'debug'))
if (-not (Test-Path $exe)) {
    throw "no client at $exe — build it first: cd client-rs; cargo build$($Release ? ' --release' : '')"
}

$env:VS_ROOT = $Root
$env:VS_TRUST = Join-Path $Root 'ans_stub.json'
$env:PODSHL_SERVER_URL = $ServerUrl
$env:PODSHL_INDEX_URL = $IndexUrl
if ($DebugPort -gt 0) {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
} else {
    Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
}

# One at a time. Two clients on one `VS_ROOT` write the same cache and the same
# ledger, and the second would look like the first misbehaving.
Get-Process podshl-client -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

# Minimised: on a debug build this is the console, and nobody asked to look at
# it. On a release build there is no console and the flag costs nothing.
$proc = Start-Process -FilePath $exe -WindowStyle Minimized -PassThru

Add-Type -Namespace Win -Name Fg -MemberDefinition @'
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    public delegate bool EnumProc(IntPtr h, IntPtr p);
'@

# The application window, not the console: same process, so it is found by
# asking which of the process's visible windows has a title.
function Get-AppWindow([int]$ProcessId) {
    $found = [IntPtr]::Zero
    $cb = [Win.Fg+EnumProc]{
        param($h, $p)
        $owner = 0
        [void][Win.Fg]::GetWindowThreadProcessId($h, [ref]$owner)
        if ($owner -eq $ProcessId -and [Win.Fg]::IsWindowVisible($h)) {
            $n = [Win.Fg]::GetWindowTextLength($h)
            if ($n -gt 0) {
                $sb = New-Object System.Text.StringBuilder ($n + 1)
                [void][Win.Fg]::GetWindowText($h, $sb, $sb.Capacity)
                # The console is titled with the path it was started from; the
                # application titles itself.
                if ($sb.ToString() -notmatch '\.exe$') { $script:found = $h; return $false }
            }
        }
        return $true
    }
    [void][Win.Fg]::EnumWindows($cb, [IntPtr]::Zero)
    return $script:found
}

$deadline = (Get-Date).AddSeconds(20)
do {
    Start-Sleep -Milliseconds 300
    $h = Get-AppWindow $proc.Id
} while ($h -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline)

if ($h -ne [IntPtr]::Zero) {
    [void][Win.Fg]::ShowWindow($h, 9)          # SW_RESTORE
    [void][Win.Fg]::SetForegroundWindow($h)
    Write-Output "client up (pid $($proc.Id)), window in front"
} else {
    # Said rather than assumed: a window that never appeared is the failure a
    # person most needs told about, and the process may still be starting.
    Write-Output "client started (pid $($proc.Id)) — no window found yet"
}
if ($DebugPort -gt 0) { Write-Output "devtools protocol on 127.0.0.1:$DebugPort" }
