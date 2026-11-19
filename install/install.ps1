#Requires -Version 5.1
<#
codegraph installer - Windows (x64 and arm64).

One-liner:
  irm https://github.com/ravijaanthony/codegraph/releases/latest/download/install.ps1 | iex

What it does, always printed as it happens:
  1. detects CPU architecture
  2. resolves which release to install (-Version, else latest)
  3. downloads the binary + checksums.txt to a temp dir
  4. verifies the SHA-256 - aborts on mismatch, installs nothing
  5. installs to $env:LOCALAPPDATA\codegraph\bin (or -InstallRoot) - no admin rights
  6. offers to add it to the User PATH (interactive; skipped when piped)

Switches:
  -Version X.Y.Z     install a specific version instead of latest
  -Uninstall         remove $env:LOCALAPPDATA\codegraph (or -InstallRoot) entirely
  -ReleaseBase URL   base URL of releases (mirrors/tests)
  -InstallRoot DIR   install directory (default: $env:LOCALAPPDATA\codegraph)
  -Target TRIPLE     override the release target triple (testing)

Windows binaries are per-arch MSVC builds; checksum-verified before install.
#>
[CmdletBinding()]
param(
    [string]$Version,
    [switch]$Uninstall,
    [string]$ReleaseBase,
    [string]$InstallRoot,
    [string]$Target
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$DefaultBase = 'https://github.com/ravijaanthony/codegraph'
$Base = if ($ReleaseBase) { $ReleaseBase } elseif ($env:CODEGRAPH_RELEASE_BASE) { $env:CODEGRAPH_RELEASE_BASE } else { $DefaultBase }
$Root = if ($InstallRoot) { $InstallRoot } elseif ($env:CODEGRAPH_INSTALL_DIR) { $env:CODEGRAPH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'codegraph' }
$BinDir = Join-Path $Root 'bin'
$BinPath = Join-Path $BinDir 'codegraph.exe'

function Say([string]$msg) { Write-Host "codegraph installer: $msg" }
function Fail([string]$msg) { Write-Error "codegraph installer: ERROR: $msg"; exit 1 }

if ($Uninstall) {
    if (Test-Path $Root) {
        Say "removing $Root"
        Remove-Item -Recurse -Force $Root
        Say "done. If $BinDir is on your User PATH, remove it via System Properties > Environment Variables."
    } else {
        Say "nothing to uninstall ($Root does not exist)"
    }
    exit 0
}

if ($env:OS -ne 'Windows_NT') { Fail "this is the Windows installer; on macOS/Linux use install.sh" }

# --- 1. Architecture ----------------------------------------------------------
if (-not $Target) {
    $archEnv = $env:PROCESSOR_ARCHITEW6432; if (-not $archEnv) { $archEnv = $env:PROCESSOR_ARCHITECTURE }
    switch -Regex ($archEnv) {
        '^(AMD64|x64|x86_64)$' { $Target = 'x86_64-pc-windows-msvc' }
        '^(ARM64|aarch64)$'    { $Target = 'aarch64-pc-windows-msvc' }
        default { Fail "unsupported architecture: $archEnv" }
    }
}
$Asset = "codegraph-$Target.exe"
Say "target: $Target"

# --- 2. Which release ----------------------------------------------------------
if ($Version) {
    $Tag = if ($Version.StartsWith('v')) { $Version } else { "v$Version" }
    Say "installing pinned version $Tag"
} else {
    Say "resolving latest release from $Base"
    try {
        $req = [System.Net.HttpWebRequest]::Create("$Base/releases/latest")
        $req.AllowAutoRedirect = $false
        $req.Timeout = 30000
        $resp = $req.GetResponse()
        $location = $resp.Headers['Location']
        $resp.Close()
    } catch {
        $resp = $_.Exception.Response
        if ($resp) { $location = $resp.Headers['Location'] } else { $location = $null }
    }
    if (-not $location) { Fail "could not resolve latest release from $Base (offline? no releases yet?)" }
    $Tag = ($location -split '/tag/')[-1].Trim('/').Trim()
    Say "latest release is $Tag"
}

$Download = "$Base/releases/download/$Tag"
$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("codegraph-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $TmpDir | Out-Null
$TmpBin = Join-Path $TmpDir $Asset
$TmpSums = Join-Path $TmpDir 'checksums.txt'

try {
    # --- 3+4. Download and verify --------------------------------------------
    Say "downloading $Download/$Asset"
    Invoke-WebRequest -Uri "$Download/$Asset" -OutFile $TmpBin -UseBasicParsing || Fail "download failed"
    Say "downloading checksums.txt"
    Invoke-WebRequest -Uri "$Download/checksums.txt" -OutFile $TmpSums -UseBasicParsing || Fail "download failed"

    $expected = $null
    foreach ($line in Get-Content $TmpSums) {
        $parts = $line -split '\s+' | Where-Object { $_ }
        if ($parts.Count -ge 2 -and ($parts[1].TrimStart('*')) -eq $Asset) { $expected = $parts[0].ToLower(); break }
    }
    if (-not $expected) { Fail "checksums.txt has no entry for $Asset - aborting, nothing was installed" }

    $actual = (Get-FileHash -Path $TmpBin -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) {
        Fail "checksum mismatch for $Asset (expected $expected, got $actual) - refusing to install; nothing was installed"
    }
    Say "SHA-256 verified ($actual)"

    # --- 5. Install -------------------------------------------------------------
    Say "installing to $BinDir"
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    $TmpNew = Join-Path $BinDir 'codegraph.exe.new'
    Copy-Item -Force $TmpBin $TmpNew
    Move-Item -Force $TmpNew $BinPath
    Say "installed $BinPath"

    # --- 6. PATH ------------------------------------------------------------------
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -contains $BinDir) {
        Say "$BinDir is already on the User PATH"
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
            Say "added to User PATH - open a new terminal to pick it up"
        } else {
            Say "not added. To add later, run:"
            Say "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$BinDir', 'User')"
        }
    }

    Say "done. Try: codegraph --version; codegraph index C:\path\to\repo"
} finally {
    Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
}
