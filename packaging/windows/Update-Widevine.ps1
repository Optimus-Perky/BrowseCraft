<#
.SYNOPSIS
Keeps AltBrowse's Widevine module (the DRM that Netflix, Audible, Spotify... need) current.

.DESCRIPTION
AltBrowse cannot ship Widevine: it is Google's, and only licensed browsers may redistribute
it. Every Windows PC already has it inside Microsoft Edge (or Chrome), so this copies the
newest one on the machine into AltBrowse's WidevineCdm folder, beside chrome.dll, where
AltBrowse loads it from at start.

  -Install    copy it now and register the "AltBrowse Widevine" task (at startup and daily,
              as SYSTEM), so an Edge update reaches AltBrowse too. Run by the installer.
  -Uninstall  remove the task and the copied module. Run by the uninstaller.
  (neither)   copy it if the machine has a newer one than AltBrowse. Run by the task.

A copy that AltBrowse has open is left whole and tried again in an hour (a one-off trigger
on the task). Logs to %ProgramData%\BrowseCraft\widevine.log (AltBrowse was BrowseCraft until 20 Nov 2026;
the folder keeps the old name, as the profile folder does). -Install and -Uninstall also
remove the old "BrowseCraft Widevine" task, if an older install left it.
#>
param([switch]$Install, [switch]$Uninstall, [string]$TaskName = 'AltBrowse Widevine')

$ErrorActionPreference = 'Stop'
$App = $PSScriptRoot
$Dest = Join-Path $App 'WidevineCdm'
$Task = $TaskName
$script:Deferred = $false
$LogDir = Join-Path $env:ProgramData 'BrowseCraft'   # kept: the old name (see above)
$OldTask = 'BrowseCraft Widevine'                      # the task's name before the rename

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
    # A previous swap's old folder goes once nothing holds it any more (AltBrowse keeps the dll it
    # loaded open from there until it restarts).
    Remove-Item "$Dest.old" -Recurse -Force -ErrorAction SilentlyContinue
    $have = ModuleVersion $Dest
    if ($have -and $have -ge $best.Version) { return }
    # Into a new folder, then swapped in, so AltBrowse never sees half a module. The old folder is
    # RENAMED aside first (allowed while AltBrowse has the dll loaded: it keeps using its copy
    # there; if a file is open in a way that blocks the rename, nothing changes and it is tried
    # again in an hour). (Deleting it file by file, as before 14 Nov 2026, removed manifest.json
    # and the .sig and left only the locked dll: Widevine broken until the task ran again.)
    $new = "$Dest.new"; $old = "$Dest.old"
    Remove-Item $new -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path $old) {   # still held from an earlier swap: no second swap on top of it yet
        $script:Deferred = $true; Log 'an earlier Widevine module is still in use; trying again in an hour'; return
    }
    Copy-Item $best.Dir $new -Recurse -Force
    try {
        if (Test-Path $Dest) { Rename-Item $Dest (Split-Path $old -Leaf) }
    } catch {
        Remove-Item $new -Recurse -Force -ErrorAction SilentlyContinue
        $script:Deferred = $true
        Log "AltBrowse has Widevine open; module left as it was, trying again in an hour ($($_.Exception.Message))"
        return
    }
    Rename-Item $new (Split-Path $Dest -Leaf)
    Remove-Item $old -Recurse -Force -ErrorAction SilentlyContinue
    Log ("Widevine {0} from {1} (was {2})" -f $best.Version, $best.Dir, $(if ($have) { $have } else { 'none' }))
}

function RetryTrigger { New-ScheduledTaskTrigger -Once -At (Get-Date).AddHours(1) }

function Add-Retry {
    # The task's own startup and daily triggers stay; one one-off trigger an hour from now replaces
    # any earlier one-off.
    $t = Get-ScheduledTask -TaskName $Task -ErrorAction SilentlyContinue
    if (-not $t) { return }
    $keep = @($t.Triggers | Where-Object { $_.CimClass.CimClassName -ne 'MSFT_TaskTimeTrigger' })
    Set-ScheduledTask -TaskName $Task -Trigger ($keep + (RetryTrigger)) | Out-Null
    Log 'retry in an hour added to the task'
}

if ($Uninstall) {
    Unregister-ScheduledTask -TaskName $Task -Confirm:$false -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $OldTask -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item $Dest, "$Dest.new" -Recurse -Force -ErrorAction SilentlyContinue
    Log 'removed (AltBrowse uninstalled)'
    exit 0
}

try { Update } catch { Log "update failed: $($_.Exception.Message)" }
if ($script:Deferred -and -not $Install) { try { Add-Retry } catch { Log "could not add the retry: $($_.Exception.Message)" } }

if ($Install) {
    if ($Task -ne $OldTask) { Unregister-ScheduledTask -TaskName $OldTask -Confirm:$false -ErrorAction SilentlyContinue }
    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $action = New-ScheduledTaskAction -Execute $ps -Argument ('-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}"' -f (Join-Path $App 'Update-Widevine.ps1'))
    $triggers = @((New-ScheduledTaskTrigger -AtStartup), (New-ScheduledTaskTrigger -Daily -At 12:30))
    if ($script:Deferred) { $triggers += RetryTrigger }
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    Register-ScheduledTask -TaskName $Task -Action $action -Trigger $triggers -Settings $settings -Principal $principal `
        -Description 'Copies the newest Widevine module on this PC (from Edge or Chrome) into AltBrowse.' -Force | Out-Null
    Log 'task registered'
}
exit 0
