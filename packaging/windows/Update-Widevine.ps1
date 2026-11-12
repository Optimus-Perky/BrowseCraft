<#
.SYNOPSIS
Keeps BrowseCraft's Widevine module (the DRM that Netflix, Audible, Spotify... need) current.

.DESCRIPTION
BrowseCraft cannot ship Widevine: it is Google's, and only licensed browsers may redistribute
it. Every Windows PC already has it inside Microsoft Edge (or Chrome), so this copies the
newest one on the machine into BrowseCraft's WidevineCdm folder, beside chrome.dll, where
BrowseCraft loads it from at start.

  -Install    copy it now and register the "BrowseCraft Widevine" task (at startup and daily,
              as SYSTEM), so an Edge update reaches BrowseCraft too. Run by the installer.
  -Uninstall  remove the task and the copied module. Run by the uninstaller.
  (neither)   copy it if the machine has a newer one than BrowseCraft. Run by the task.

A copy that BrowseCraft has open is left alone and tried again next time. Logs to
%ProgramData%\BrowseCraft\widevine.log.
#>
param([switch]$Install, [switch]$Uninstall)

$ErrorActionPreference = 'Stop'
$App = $PSScriptRoot
$Dest = Join-Path $App 'WidevineCdm'
$Task = 'BrowseCraft Widevine'
$LogDir = Join-Path $env:ProgramData 'BrowseCraft'

function Log($text) {
    try {
        New-Item -ItemType Directory -Path $LogDir -Force | Out-Null
        Add-Content -Path (Join-Path $LogDir 'widevine.log') -Value ("{0:yyyy-MM-dd HH:mm:ss} {1}" -f (Get-Date), $text)
    } catch { }
}

function ModuleVersion($dir) {
    try {
        $m = Get-Content (Join-Path $dir 'manifest.json') -Raw | ConvertFrom-Json
        if (-not (Test-Path (Join-Path $dir '_platform_specific\win_x64\widevinecdm.dll'))) { return $null }
        return [version]$m.version
    } catch { return $null }
}

# Every browser on the machine that carries one: Edge (and its EdgeCore copies) and Chrome,
# in their version folders. Repair copies (Optimized, ResiliencyLinks) are skipped.
function Candidates {
    $roots = @(
        "${env:ProgramFiles(x86)}\Microsoft\Edge\Application"
        "${env:ProgramFiles(x86)}\Microsoft\EdgeCore"
        "$env:ProgramFiles\Microsoft\Edge\Application"
        "$env:ProgramFiles\Google\Chrome\Application"
        "${env:ProgramFiles(x86)}\Google\Chrome\Application"
    )
    foreach ($r in $roots) {
        if (-not (Test-Path $r)) { continue }
        Get-ChildItem $r -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^\d+(\.\d+){2,3}$' } |
            ForEach-Object { Join-Path $_.FullName 'WidevineCdm' } |
            Where-Object { Test-Path $_ }
    }
}

function Update {
    $best = Candidates | ForEach-Object { [pscustomobject]@{ Dir = $_; Version = ModuleVersion $_ } } |
        Where-Object Version | Sort-Object Version -Descending | Select-Object -First 1
    if (-not $best) { Log 'no Widevine module found on this machine (no Edge or Chrome?)'; return }
    $have = ModuleVersion $Dest
    if ($have -and $have -ge $best.Version) { return }
    # Into a new folder, then swapped in, so BrowseCraft never sees half a module.
    $new = "$Dest.new"
    Remove-Item $new -Recurse -Force -ErrorAction SilentlyContinue
    Copy-Item $best.Dir $new -Recurse -Force
    try {
        if (Test-Path $Dest) { Remove-Item $Dest -Recurse -Force }
        Rename-Item $new (Split-Path $Dest -Leaf)
        Log ("Widevine {0} from {1} (was {2})" -f $best.Version, $best.Dir, $(if ($have) { $have } else { 'none' }))
    } catch {
        Remove-Item $new -Recurse -Force -ErrorAction SilentlyContinue
        Log "BrowseCraft has Widevine open; trying again next time ($($_.Exception.Message))"
    }
}

if ($Uninstall) {
    Unregister-ScheduledTask -TaskName $Task -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item $Dest, "$Dest.new" -Recurse -Force -ErrorAction SilentlyContinue
    Log 'removed (BrowseCraft uninstalled)'
    exit 0
}

try { Update } catch { Log "update failed: $($_.Exception.Message)" }

if ($Install) {
    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $action = New-ScheduledTaskAction -Execute $ps -Argument ('-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}"' -f (Join-Path $App 'Update-Widevine.ps1'))
    $triggers = @((New-ScheduledTaskTrigger -AtStartup), (New-ScheduledTaskTrigger -Daily -At 12:30))
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    Register-ScheduledTask -TaskName $Task -Action $action -Trigger $triggers -Settings $settings -Principal $principal `
        -Description 'Copies the newest Widevine module on this PC (from Edge or Chrome) into BrowseCraft.' -Force | Out-Null
    Log 'task registered'
}
exit 0
