#Requires -Version 5.1
<#
codegraph installer - Windows (x64 and arm64).

One-liner (public releases only):
  irm https://github.com/BuildTeamRYS/codegraph/releases/latest/download/install.ps1 | iex

IMPORTANT: this repository is private. Both remote modes (latest and -Version)
need a GitHub token, or a download from your logged-in browser. For
private/offline use, download the release assets manually, or use a pre-made
bundle, then install with:
  powershell -ExecutionPolicy Bypass -File install.ps1 -FromDir .\assets
(see README.md "Install (private demo)").

What it does, always printed as it happens:
  1. detects CPU architecture
  2. resolves which release to install (-Version, -FromDir, else latest)
  3. downloads the binary + checksums.txt (or finds them in the folder)
  4. verifies the SHA-256 - aborts on mismatch, installs nothing
  5. installs to $env:LOCALAPPDATA\codegraph\bin (or -InstallRoot) - no admin rights
  6. offers to add it to the User PATH; prints exactly what went where

Switches:
  -Version X.Y.Z     install a specific version instead of latest
  -FromDir DIR       install from a local folder of release assets (offline)
  -SkipChecksum      from-dir only: install even without a checksums.txt
  -Target TRIPLE     override the release target triple (testing)
  -Uninstall         remove $env:LOCALAPPDATA\codegraph (or -InstallRoot); prints the
                     PATH entry to remove and offers to undo agent configs
  -Yes               with -Uninstall: skip the confirmation prompts
  -ReleaseBase URL   base URL of releases (mirrors/tests)
  -InstallRoot DIR   install directory (default: $env:LOCALAPPDATA\codegraph)
#>
[CmdletBinding()]
param(
    [string]$Version,
    [switch]$Uninstall,
    [switch]$Yes,
    [switch]$SkipChecksum,
    [string]$FromDir,
    [string]$ReleaseBase,
    [string]$InstallRoot,
    [string]$Target
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$DefaultBase = 'https://github.com/BuildTeamRYS/codegraph'
$Base = if ($ReleaseBase) { $ReleaseBase } elseif ($env:CODEGRAPH_RELEASE_BASE) { $env:CODEGRAPH_RELEASE_BASE } else { $DefaultBase }
$Root = if ($InstallRoot) { $InstallRoot } elseif ($env:CODEGRAPH_INSTALL_DIR) { $env:CODEGRAPH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'codegraph' }
$BinDir = Join-Path $Root 'bin'
$BinPath = Join-Path $BinDir 'codegraph.exe'

function Say([string]$msg) { Write-Host "codegraph installer: $msg" }
function Fail([string]$msg) { Write-Error "codegraph installer: ERROR: $msg"; exit 1 }

function Final-Summary {
    $installedVersion = ''
    try {
        $vi = (& $BinPath --version 2>$null) -split '\s+' | Select-Object -Last 1
        if ($vi) { $installedVersion = " (codegraph $vi)" }
    } catch {}
    Write-Host ""
    Say "done."
    Say "  installed (uninstall with: .\install.ps1 -Uninstall):"
    Say "    $BinPath$installedVersion"
    Say "  agent configs created by 'codegraph init' point at this exact path"
    Say "  undo everything:"
    Say "    1) codegraph agents remove <id>   # per agent you configured (or 'codegraph agents list')"
    Say "    2) .\install.ps1 -Uninstall"
    Say "  try it: codegraph --version; codegraph doctor"
}

if ($Uninstall) {
    Write-Host ""
    Say "[1/3] agent configs"
    if (Test-Path $BinPath) {
        Write-Host "--- codegraph agents list (current folder) ---"
        & $BinPath agents list 2>&1 | Write-Host
        $answer = 'no'
        if (-not $Yes) {
            if ([Environment]::UserInteractive) {
                $answer = Read-Host "remove codegraph MCP entries from configured agents now? [y/N]"
            }
        } else { $answer = 'yes' }
        if ($answer -match '^(y|yes)$') {
            foreach ($id in @('claude-code','cursor','vscode-copilot','codex-cli','gemini-cli','antigravity','opencode','windsurf','claude-desktop')) {
                & $BinPath agents remove $id 2>&1 | Write-Host
            }
        } else {
            Say "skipping agent removal - do it later with: codegraph agents remove <id>"
        }
    } else {
        Say "$BinPath not present - nothing agent-side to do"
    }
    Write-Host ""
    Say "[2/3] remove install root"
    if (Test-Path $Root) {
        Say "removing $Root"
        Remove-Item -Recurse -Force $Root
        Say "removed."
    } else {
        Say "nothing to uninstall ($Root does not exist)"
    }
    Write-Host ""
    Say "[3/3] PATH cleanup"
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -and (($userPath -split ';') -contains $BinDir)) {
        if ($Yes -or ([Environment]::UserInteractive -and ((Read-Host "remove $BinDir from your User PATH now? [y/N]") -match '^(y|yes)$'))) {
            $newPath = (($userPath -split ';') | Where-Object { $_ -and ($_ -ne $BinDir) }) -join ';'
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Say "removed from User PATH (open a new terminal to pick up the change)"
        } else {
            Say "not removed. To remove later, run:"
            Say "  [Environment]::SetEnvironmentVariable('Path', ([Environment]::GetEnvironmentVariable('Path','User') -split ';' | Where-Object { `$_ -and `$_ -ne '$BinDir' }) -join ';', 'User')"
        }
    } else {
        Say "$BinDir is not on the User PATH - nothing to clean"
    }
    exit 0
}

if ($env:OS -ne 'Windows_NT') { Fail "this is the Windows installer; on macOS/Linux use install.sh" }

# --- 1. Architecture ----------------------------------------------------------
Say "[1/6] detecting platform"
if (-not $Target) {
    $archEnv = $env:PROCESSOR_ARCHITEW6432; if (-not $archEnv) { $archEnv = $env:PROCESSOR_ARCHITECTURE }
    switch -Regex ($archEnv) {
        '^(AMD64|x64|x86_64)$' { $Target = 'x86_64-pc-windows-msvc' }
        '^(ARM64|aarch64)$'    { $Target = 'aarch64-pc-windows-msvc' }
        default { Fail "unsupported architecture: $archEnv" }
    }
}
$Asset = "codegraph-$Target.exe"
Say "  Windows $env:PROCESSOR_ARCHITECTURE -> target $Target (asset: $Asset)"

# --- 2. Which release ----------------------------------------------------------
Say "[2/6] resolving release"
if ($FromDir) {
    if (-not (Test-Path $FromDir)) { Fail "-FromDir: not a directory: $FromDir" }
    Say "  offline mode: reading assets from $FromDir"
} elseif ($Version) {
    $Tag = if ($Version.StartsWith('v')) { $Version } else { "v$Version" }
    Say "  pinned version -> tag $Tag"
} else {
    Say "  resolving latest release from $Base (private repos 404 here; use -FromDir or a browser download)"
    $Tag = $null
    try {
        $req = [System.Net.HttpWebRequest]::Create("$Base/releases/latest")
        $req.AllowAutoRedirect = $false
        $req.Timeout = 30000
        $resp = $req.GetResponse()
        $location = $resp.Headers['Location']
        $resp.Close()
        if ($location) { $Tag = ($location -split '/tag/')[-1].Trim('/').Trim() }
    } catch {
        $resp = $_.Exception.Response
        if ($resp -and $resp.Headers['Location']) { $Tag = ($resp.Headers['Location'] -split '/tag/')[-1].Trim('/').Trim() }
    }
    if (-not $Tag) { Fail "could not resolve latest release from $Base - the repo is private or has no releases; use -FromDir or download assets via a browser (see README)" }
    Say "  latest release: $Tag"
}

# ARM64 fallback: if the ARM64 asset does not exist, transparently use the
# x86_64 asset (Windows emulates x86_64 on ARM64).
$FallbackUsed = $false
if ($Target -eq 'aarch64-pc-windows-msvc' -and -not $FromDir) {
    $DownloadProbe = "$Base/releases/download/$Tag/$Asset"
    try {
        $probe = [System.Net.WebRequest]::Create($DownloadProbe)
        $probe.Method = 'HEAD'
        $probe.Timeout = 15000
        $pr = $probe.GetResponse(); $pr.Close()
    } catch {
        if ($_.Exception.Response -and $_.Exception.Response.StatusCode.value__ -eq 404) {
            Say "  note: ARM64 asset not found in release - falling back to x86_64 (Windows emulates it)"
            $Target = 'x86_64-pc-windows-msvc'; $Asset = "codegraph-$Target.exe"; $FallbackUsed = $true
        }
    }
}

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("codegraph-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $TmpDir | Out-Null
$TmpBin = Join-Path $TmpDir $Asset
$TmpSums = Join-Path $TmpDir 'checksums.txt'

try {
    if ($FromDir) {
        # --- stage from folder -----------------------------------------------
        Say "[3/6] staging from $FromDir"
        $srcBin = $null
        $srcSum = $null
        if (Test-Path (Join-Path $FromDir $Asset)) {
            if ($Target -eq 'aarch64-pc-windows-msvc' -and (Get-Item (Join-Path $FromDir $Asset)).Length -eq 0) {
                $srcBin = $null
            } else { $srcBin = Join-Path $FromDir $Asset }
        }
        if (-not $srcBin -and $Target -eq 'aarch64-pc-windows-msvc' -and (Test-Path (Join-Path $FromDir 'codegraph-x86_64-pc-windows-msvc.exe'))) {
            Say "  note: ARM64 asset not found in folder - falling back to x86_64 (Windows emulates it)"
            $Target = 'x86_64-pc-windows-msvc'; $Asset = 'codegraph-x86_64-pc-windows-msvc.exe'; $FallbackUsed = $true
            $srcBin = Join-Path $FromDir $Asset
        }
        if (-not $srcBin) { Fail "-FromDir: missing asset $Asset in $FromDir" }
        $srcSum = Join-Path $FromDir 'checksums.txt'
        Say "  found binary: $srcBin ($((Get-Item $srcBin).Length) bytes)"
        Copy-Item $srcBin $TmpBin
        if (Test-Path $srcSum) {
            Say "  found checksums.txt - verifying"
            Copy-Item $srcSum $TmpSums
        } elseif ($SkipChecksum) {
            Say "  no checksums.txt present and -SkipChecksum given - installing unverified"
        } else {
            Fail "-FromDir: no checksums.txt in $FromDir (pass -SkipChecksum to install anyway)"
        }
    } else {
        # --- 3+4. Download -----------------------------------------------------
        $Download = "$Base/releases/download/$Tag"
        Say "[3/6] downloading (progress with ETA below)"
        Say "  $Download/$Asset"
        $timer = [Diagnostics.Stopwatch]::StartNew()

        $client = New-Object System.Net.Http.HttpClient
        try {
            $req = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, "$Download/$Asset")
            $resp = $client.SendAsync($req, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
            if (-not $resp.IsSuccessStatusCode) {
                Fail "download failed ($([int]$resp.StatusCode) $($resp.ReasonPhrase)) - private repo? use -FromDir or a browser download (see README)"
            }
            $total = [int64]($resp.Content.Headers.ContentLength | ForEach-Object { $_ })
            if (-not $total) { $total = 0 }
            $stream = $resp.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
            $fs = [System.IO.File]::Create($TmpBin)
            $buf = New-Object byte[] 65536
            $read = [int64]0
            while (($n = $stream.Read($buf, 0, $buf.Length)) -gt 0) {
                $fs.Write($buf, 0, $n)
                $read += $n
                if ($total -gt 0) {
                    $pct = [int](($read * 100) / $total)
                    $el = $timer.Elapsed.TotalSeconds
                    $remain = 0
                    if ($read -gt 0 -and $el -gt 0) { $remain = [int](($total - $read) / ($read / $el)) }
                    Write-Progress -Activity "codegraph installer: downloading $Asset" `
                        -Status ("{0:N1} MB / {1:N1} MB" -f ($read / 1MB), ($total / 1MB)) `
                        -PercentComplete $pct -SecondsRemaining $remain
                }
            }
            $fs.Close()
            Write-Progress -Activity "codegraph installer: downloading $Asset" -Completed
            $stream.Dispose()
        } finally {
            $client.Dispose()
        }
        $timer.Stop()
        $size = (Get-Item $TmpBin).Length
        Say "  downloaded $([math]::Round($size / 1MB, 2)) MB in $([math]::Round($timer.Elapsed.TotalSeconds, 1))s"
        Say "  downloading checksums.txt"
        Invoke-WebRequest -Uri "$Download/checksums.txt" -OutFile $TmpSums -UseBasicParsing || Fail "download failed: $Download/checksums.txt"
    }

    # --- 4. Verify ------------------------------------------------------------
    Say "[4/6] verifying SHA-256"
    $expected = $null
    if (Test-Path $TmpSums) {
        foreach ($line in Get-Content $TmpSums) {
            $parts = $line -split '\s+' | Where-Object { $_ }
            if ($parts.Count -ge 2 -and ($parts[1].TrimStart('*')) -eq $Asset) { $expected = $parts[0].ToLower(); break }
        }
    }
    if (-not $expected) {
        if ($FromDir -and $SkipChecksum) {
            Say "  no checksum entry - skipping verification (-SkipChecksum)"
        } else {
            Fail "checksums.txt has no entry for $Asset - aborting, nothing was installed"
        }
    } else {
        $actual = (Get-FileHash -Path $TmpBin -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $expected) {
            Fail "checksum mismatch for $Asset (expected $expected, got $actual) - refusing to install; nothing was installed"
        }
        Say "  SHA-256 verified ($actual)"
    }

    # --- 5. Install -------------------------------------------------------------
    Say "[5/6] installing"
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    $TmpNew = Join-Path $BinDir 'codegraph.exe.new'
    Copy-Item -Force $TmpBin $TmpNew
    Move-Item -Force $TmpNew $BinPath
    Say "  installed $BinPath (static MSVC binary - no system dependencies)"
    if ($FromDir) { Say "  source: $FromDir" } else { Say "  source: $Base/releases/download/$Tag$(if ($FallbackUsed) {' (x86_64 via emulation on ARM64 Windows)'} else {''})" }

    # --- 6. PATH ------------------------------------------------------------------
    Say "[6/6] PATH"
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -contains $BinDir) {
        Say "  $BinDir is already on the User PATH"
    } else {
        $added = $false
        try {
            if ([Environment]::UserInteractive) {
                $answer = Read-Host "codegraph installer: add $BinDir to your User PATH now? [y/N]"
                if ($answer -match '^(y|yes)$') { $added = $true }
            }
        } catch { $added = $false }
        if ($added) {
            $newPath = if ([string]::IsNullOrWhiteSpace($userPath)) { $BinDir } else { "$($userPath.TrimEnd(';'));$BinDir" }
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Say "  added to User PATH - open a new terminal to pick it up"
        } else {
            Say "  not added. To add later, run:"
            Say "    [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$BinDir', 'User')"
        }
    }

    Final-Summary
} finally {
    Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
}
