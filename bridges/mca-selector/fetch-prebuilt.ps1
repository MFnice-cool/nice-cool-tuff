# Populate bridges/mca-selector/prebuilt with MCA Selector JAR + OpenJFX (Windows).
# Run from repo root: powershell -File bridges/mca-selector/fetch-prebuilt.ps1

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Pre = Join-Path $Root "prebuilt"
$Lib = Join-Path $Pre "javafx-lib"
New-Item -ItemType Directory -Force -Path $Lib | Out-Null

$McaVer = "2.8"
$JarName = "mcaselector-$McaVer.jar"
$JarUrl = "https://github.com/Querz/mcaselector/releases/download/$McaVer/$JarName"
$JarOut = Join-Path $Pre $JarName

if (-not (Test-Path $JarOut) -or (Get-Item $JarOut).Length -lt 1MB) {
  Write-Host "Downloading $JarName..."
  curl.exe -L --fail --retry 3 -o $JarOut $JarUrl
}

$FxVer = "21.0.6"
$Mods = @("base", "graphics", "controls", "fxml", "swing", "media", "web")
foreach ($m in $Mods) {
  $name = "javafx-$m-$FxVer-win.jar"
  $out = Join-Path $Lib $name
  if ((Test-Path $out) -and (Get-Item $out).Length -gt 10KB) {
    Write-Host "OK $name"
    continue
  }
  $url = "https://repo1.maven.org/maven2/org/openjfx/javafx-$m/$FxVer/$name"
  Write-Host "Downloading $name..."
  curl.exe -L --fail --retry 3 -o $out $url
}

Write-Host "Prebuilt ready:"
Get-ChildItem $Pre -Recurse -File | Select-Object FullName, Length
