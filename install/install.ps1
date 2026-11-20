$ErrorActionPreference = "Stop"

$Repo = "BuildTeamRYS/codegraph"
$RepoUrl = "https://github.com/$Repo.git"
$BinName = "codegraph.exe"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\codegraph"

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

# 1. Install Rust via rustup if cargo is not available (using GNU ABI to avoid needing MSVC Build Tools)
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    if (-not (Test-Path (Join-Path $CargoBin "cargo.exe"))) {
        Write-Host "Rust not found. Installing Rust toolchain via rustup..."
        $RustupInit = Join-Path $env:TEMP "rustup-init.exe"
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $RustupInit
        & $RustupInit -y --default-host x86_64-pc-windows-gnu --profile minimal --no-modify-path
        Remove-Item $RustupInit -Force
    }
    $env:Path = "$CargoBin;$env:Path"
}

# 2. Download source archive and compile with Cargo
$BuildDir = Join-Path $env:TEMP "codegraph-build-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null

try {
    $SrcZip = Join-Path $BuildDir "src.zip"
    Write-Host "Downloading source from https://github.com/$Repo..."
    Invoke-WebRequest -Uri "https://github.com/$Repo/archive/refs/heads/main.zip" -OutFile $SrcZip

    Expand-Archive -Path $SrcZip -DestinationPath $BuildDir -Force
    $ExtractedDir = Get-ChildItem -Path $BuildDir -Directory | Select-Object -First 1

    Write-Host "Compiling codegraph (this may take a couple of minutes)..."
    Push-Location $ExtractedDir.FullName
    try {
        cargo build --release --locked
    } finally {
        Pop-Location
    }

    $BuiltExe = Join-Path $ExtractedDir.FullName "target\release\$BinName"
    if (-not (Test-Path $BuiltExe)) {
        $FoundExe = Get-ChildItem -Path (Join-Path $ExtractedDir.FullName "target\release") -Filter "*.exe" | Select-Object -First 1
        if ($FoundExe) {
            $BuiltExe = $FoundExe.FullName
        } else {
            throw "Build succeeded, but no compiled .exe was found in target\release."
        }
    }

    Copy-Item -Path $BuiltExe -Destination (Join-Path $InstallDir $BinName) -Force
} finally {
    Remove-Item -Path $BuildDir -Recurse -Force -ErrorAction SilentlyContinue
}

# 3. Add InstallDir to User PATH if not already present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    $NewPath = if ([string]::IsNullOrWhiteSpace($UserPath)) { $InstallDir } else { "$UserPath;$InstallDir" }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$env:Path;$InstallDir"
}

Write-Host "Installed codegraph to $InstallDir. Open a new terminal and run: codegraph --help"
