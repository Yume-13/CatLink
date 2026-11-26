# MIT License
#
# Copyright © 2026 Yume-13
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.

# Builds the Windows client and the Android app into .\dist and packages
# .\CatLink-<version>-windows.zip.
#
#   .\build.ps1 [-Version 0.1.0]
#
# Needs Rust, JDK 17 (JAVA_HOME) and an Android SDK (ANDROID_HOME, default
# %LOCALAPPDATA%\Android\Sdk) with platform 35, build-tools and platform-tools.
#
# The APK is signed with the key in CATLINK_KEYSTORE (with CATLINK_KEYSTORE_PASSWORD,
# CATLINK_KEY_ALIAS, CATLINK_KEY_PASSWORD) when set, otherwise with the debug key.
param([string]$Version = '0.1.0')
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must be MAJOR.MINOR.PATCH, got '$Version'." }
$env:CATLINK_VERSION = $Version
$root = $PSScriptRoot
if (-not $env:ANDROID_HOME) {
    $sdk = Join-Path $env:LOCALAPPDATA 'Android\Sdk'
    if (Test-Path $sdk) { $env:ANDROID_HOME = $sdk }
}
if (-not $env:JAVA_HOME) { throw 'JAVA_HOME is not set (JDK 17 needed for the Android build).' }
if (-not $env:ANDROID_HOME) { throw 'ANDROID_HOME is not set (Android SDK needed).' }
$env:PATH = "$env:JAVA_HOME\bin;$env:PATH"

# Wintun 0.14.1 (official build, verified by SHA-256).
$vendor = Join-Path $root 'windows\vendor'
$wintunDll = Join-Path $vendor 'wintun.dll'
$wintunLicense = Join-Path $vendor 'wintun-LICENSE.txt'
if (-not (Test-Path $wintunDll) -or -not (Test-Path $wintunLicense)) {
    Write-Host '== Wintun' -ForegroundColor Cyan
    $zip = Join-Path $env:TEMP 'wintun-0.14.1.zip'
    Invoke-WebRequest 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $zip -UseBasicParsing
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
    if ($hash -ne '07C256185D6EE3652E09FA55C0B673E2624B565E02C4B9091C79CA7D2F24EF51') {
        throw "wintun-0.14.1.zip has an unexpected SHA-256: $hash"
    }
    $tmp = Join-Path $env:TEMP 'wintun-0.14.1'
    Expand-Archive $zip $tmp -Force
    New-Item -ItemType Directory -Force $vendor | Out-Null
    Copy-Item (Join-Path $tmp 'wintun\bin\amd64\wintun.dll') $wintunDll -Force
    Copy-Item (Join-Path $tmp 'wintun\LICENSE.txt') $wintunLicense -Force
}

Write-Host '== Windows client' -ForegroundColor Cyan
# Replace local paths embedded in the binaries (panic locations) with neutral prefixes.
$remap = @(
    "--remap-path-prefix=$env:USERPROFILE\.cargo=cargo",
    "--remap-path-prefix=$env:USERPROFILE\.rustup=rustup",
    "--remap-path-prefix=$root=catlink"
)
$env:CARGO_ENCODED_RUSTFLAGS = $remap -join [char]0x1f
Push-Location (Join-Path $root 'windows')
cargo build --release
if ($LASTEXITCODE) { throw 'cargo build failed' }
Remove-Item Env:CARGO_ENCODED_RUSTFLAGS
# Licenses of the linked crates (windows\about.toml, windows\about.hbs).
if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    cargo install cargo-about --locked --features cli
    if ($LASTEXITCODE) { throw 'cargo install cargo-about failed' }
}
$rustNotices = Join-Path $root 'windows\target\rust-crates.txt'
cargo about generate about.hbs --output-file $rustNotices
if ($LASTEXITCODE) { throw 'cargo about failed' }
Pop-Location

Write-Host '== Android app' -ForegroundColor Cyan
Push-Location (Join-Path $root 'android')
"sdk.dir=$($env:ANDROID_HOME.Replace('\', '\\'))" | Set-Content local.properties
.\gradlew.bat assembleRelease :app:listReleaseLibraries "-PcatlinkVersion=$Version" --console=plain -q
if ($LASTEXITCODE) { throw 'gradle build failed' }
Pop-Location
$apk = Join-Path $root 'android\app\build\outputs\apk\release\app-release.apk'
# Report which certificate signed the APK.
$buildTools = Get-ChildItem (Join-Path $env:ANDROID_HOME 'build-tools') -Directory | Sort-Object { [version]($_.Name -replace '[^\d.].*$', '') } | Select-Object -Last 1
$signer = (& (Join-Path $buildTools.FullName 'apksigner.bat') verify --print-certs $apk 2>&1 | Select-String 'certificate DN:' | Select-Object -First 1).Line
if (-not $signer) { throw 'Could not verify the APK signature.' }
Write-Host "APK $($signer.Trim())"
if ($env:CATLINK_KEYSTORE -and $signer -match 'Android Debug') { throw 'A release keystore was given but the APK is debug-signed.' }

# Every library in the APK (AndroidX, Kotlin, kotlinx, JetBrains annotations, Guava
# listenablefuture) is licensed under Apache-2.0.
$apache = (Invoke-WebRequest 'https://www.apache.org/licenses/LICENSE-2.0.txt' -UseBasicParsing).Content
if ($apache -notmatch 'Version 2\.0, January 2004') { throw 'unexpected Apache-2.0 license text' }
$libraries = Get-Content (Join-Path $root 'android\app\build\outputs\release-libraries.txt')
$androidNotices = Join-Path $root 'android\app\build\outputs\android-libraries.txt'
$text = @(
    'Third-party software included in catlink.apk'
    '============================================'
    ''
    'The following libraries are licensed under the Apache License, Version 2.0,'
    'reproduced below.'
    ''
    ($libraries | ForEach-Object { "  $_" })
    ''
    '-------------------------------------------------------------------------------'
    ($apache -split "\r?\n")
) -join "`r`n"
[IO.File]::WriteAllText($androidNotices, $text, (New-Object System.Text.UTF8Encoding $false))

Write-Host '== dist' -ForegroundColor Cyan
$dist = Join-Path $root 'dist'
# Stop an adb server started from dist\ so adb.exe can be replaced.
$distAdb = Join-Path $dist 'adb.exe'
if (Test-Path $distAdb) {
    try { & $distAdb kill-server 2>&1 | Out-Null } catch { }
    Start-Sleep -Milliseconds 500
}
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dist | Out-Null
foreach ($exe in 'catlink.exe', 'catlink-cli.exe') {
    Copy-Item (Join-Path $root "windows\target\release\$exe") $dist -Force
}
Copy-Item $wintunDll $dist -Force
Copy-Item (Join-Path $root 'LICENSE.txt') $dist -Force
Copy-Item (Join-Path $root 'android\app\build\outputs\apk\release\app-release.apk') (Join-Path $dist 'catlink.apk') -Force
$pt = Join-Path $env:ANDROID_HOME 'platform-tools'
foreach ($f in 'adb.exe', 'AdbWinApi.dll', 'AdbWinUsbApi.dll', 'NOTICE.txt') {
    if (-not (Test-Path (Join-Path $pt $f))) { throw "$f not found in $pt (Android SDK platform-tools needed)." }
}
foreach ($f in 'adb.exe', 'AdbWinApi.dll', 'AdbWinUsbApi.dll') { Copy-Item (Join-Path $pt $f) $dist -Force }

# Third-party notices.
$licenses = Join-Path $dist 'licenses'
New-Item -ItemType Directory -Force $licenses | Out-Null
Copy-Item $rustNotices (Join-Path $licenses 'rust-crates.txt') -Force
Copy-Item $androidNotices (Join-Path $licenses 'android-libraries.txt') -Force
Copy-Item $wintunLicense (Join-Path $licenses 'wintun.txt') -Force
Copy-Item (Join-Path $pt 'NOTICE.txt') (Join-Path $licenses 'adb.txt') -Force
Get-ChildItem $dist -Recurse -File | ForEach-Object { '{0,-36} {1,10}' -f $_.FullName.Substring($dist.Length + 1), $_.Length }

$zipOut = Join-Path $root "CatLink-$Version-windows.zip"
# Zip entry names use '/' separators.
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
Remove-Item $zipOut -Force -ErrorAction SilentlyContinue
$zip = [System.IO.Compression.ZipFile]::Open($zipOut, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    Get-ChildItem $dist -Recurse -File | ForEach-Object {
        $entry = $_.FullName.Substring($dist.Length + 1).Replace('\', '/')
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $entry, [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally {
    $zip.Dispose()
}
Write-Host "== $zipOut" -ForegroundColor Cyan
