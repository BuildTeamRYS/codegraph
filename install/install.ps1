$ErrorActionPreference = "Stop"

$Repo = "BuildTeamRYS/codegraph"
$AssetName = "codegraph-windows-x64.zip"
$Url = "https://github.com/$Repo/releases/latest/download/$AssetName"

$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\codegraph"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$ZipPath = Join-Path $env:TEMP $AssetName
Write-Host "Downloading $Url..."
Invoke-WebRequest -Uri $Url -OutFile $ZipPath

Expand-Archive -Path $ZipPath -DestinationPath $InstallDir -Force
Remove-Item $ZipPath -Force

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
}

Write-Host "Installed codegraph to $InstallDir. Open a new terminal and run: codegraph --help"
