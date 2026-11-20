# Builds AltBrowse.exe (the installer) in C:\bcbuild from the Windows build zip and the
# files beside this script. Run by build-installer.sh over SSH.
param([Parameter(Mandatory = $true)][string]$Version, [Parameter(Mandatory = $true)][string]$BrowseCraftVersion)
$ErrorActionPreference = "Stop"
$d = "C:\bcbuild"
Set-Location $d
# The program, from the zip (one top folder), less Chromium's own installer pieces.
Expand-Archive -Path (Get-ChildItem "$d\*.zip" | Select -First 1).FullName -DestinationPath "$d\zip" -Force
$top = Get-ChildItem "$d\zip" -Directory | Select -First 1
Move-Item $top.FullName "$d\files"
Remove-Item "$d\files\setup.exe", "$d\files\mini_installer_exe_version.rc" -Force -ErrorAction SilentlyContinue

# The program carries Chromium's version; the installer adds AltBrowse's build number.
$chrome = (Get-Item "$d\files\chrome.exe").VersionInfo.FileVersion
$v = [version]$Version
if ("$($v.Major).$($v.Minor).$($v.Build)" -ne ($chrome -replace '^(\d+\.\d+\.\d+).*', '$1')) { throw "chrome.exe is $chrome; the installer is $Version" }
# Windows Installer only compares three parts; a rebuild of one Chromium version replaces itself.
$msiVersion = "$($v.Major).$($v.Minor).$($v.Build)"

Add-Type -AssemblyName System.Drawing
$src = [System.Drawing.Image]::FromFile("$d\browsecraft-logo-256.png")
$logo = New-Object System.Drawing.Bitmap 64, 64
$g = [System.Drawing.Graphics]::FromImage($logo)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.DrawImage($src, 0, 0, 64, 64); $logo.Save("$d\logo.png"); $g.Dispose(); $src.Dispose()

# Every file of the program, a component each (GUIDs made from their paths, so they stay the
# same build to build).
& C:\wix314\heat.exe dir "$d\files" -nologo -cg AppFiles -dr INSTALLDIR -ag -srd -sfrag -sreg -var var.Files -out files.wxs
if ($LASTEXITCODE -ne 0) { throw "heat failed" }
& C:\wix314\candle.exe -nologo -arch x64 "-dVersion=$Version" "-dBcVersion=$BrowseCraftVersion" "-dMsiVersion=$msiVersion" "-dBuildDir=$d" "-dFiles=$d\files" -ext WixUtilExtension -out obj\ browsecraft.wxs files.wxs
if ($LASTEXITCODE -ne 0) { throw "candle (msi) failed" }
# ICE43, ICE57: per-user data in a per-machine component owning shortcuts; they cannot see that
# InstallScope="perMachine" makes the Start menu and desktop the All Users ones.
# ICE61: the same-version upgrade (deliberate). ICE91: per-user dirs, the same false alarm.
& C:\wix314\light.exe -nologo -ext WixUtilExtension -sice:ICE43 -sice:ICE57 -sice:ICE61 -sice:ICE91 -out browsecraft.msi obj\browsecraft.wixobj obj\files.wixobj
if ($LASTEXITCODE -ne 0) { throw "light (msi) failed" }
& C:\wix314\candle.exe -nologo -arch x64 "-dVersion=$Version" "-dBuildDir=$d" -ext WixBalExtension -ext WixUtilExtension -out obj\bundle.wixobj bundle.wxs
if ($LASTEXITCODE -ne 0) { throw "candle (bundle) failed" }
& C:\wix314\light.exe -nologo -ext WixBalExtension -ext WixUtilExtension -out AltBrowse.exe obj\bundle.wixobj
if ($LASTEXITCODE -ne 0) { throw "light (bundle) failed" }
$i = Get-Item "$d\AltBrowse.exe"
"  built AltBrowse.exe (installer) {0:N0} bytes, file version {1}, browser {2}, {3} files" -f $i.Length, $i.VersionInfo.FileVersion, $chrome, (Get-ChildItem "$d\files" -Recurse -File).Count
