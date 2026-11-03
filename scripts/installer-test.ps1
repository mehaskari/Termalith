# Installs the built MSI and NSIS installers silently, launches the
# installed app with the smoke test, then uninstalls.
#
# Usage: pwsh scripts/installer-test.ps1 -Bundle target\release\bundle
param([Parameter(Mandatory = $true)][string]$Bundle)
$ErrorActionPreference = 'Stop'

function Find-App([string]$dir) {
  Get-ChildItem -Path $dir -Filter *.exe -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notmatch 'uninstall' } | Select-Object -First 1
}

$msi = Get-ChildItem "$Bundle\msi\*.msi" | Select-Object -First 1
Write-Host "== MSI: $($msi.FullName)"
$log = Join-Path $env:RUNNER_TEMP 'msi-install.log'
$p = Start-Process msiexec.exe -ArgumentList '/i', "`"$($msi.FullName)`"", '/qn', '/l*v', "`"$log`"" -Wait -PassThru
if ($p.ExitCode -ne 0) { Get-Content $log -Tail 40; throw "MSI install failed ($($p.ExitCode))" }
$exe = Find-App "$env:ProgramFiles\Termalith"
if (-not $exe) { $exe = Find-App "${env:ProgramFiles(x86)}\Termalith" }
if (-not $exe) { throw "installed app not found after the MSI install" }
Write-Host "installed: $($exe.FullName)"
& "$PSScriptRoot\smoke-test.ps1" -Bin $exe.FullName
$p = Start-Process msiexec.exe -ArgumentList '/x', "`"$($msi.FullName)`"", '/qn' -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "MSI uninstall failed ($($p.ExitCode))" }

$nsis = Get-ChildItem "$Bundle\nsis\*-setup.exe" | Select-Object -First 1
Write-Host "== NSIS: $($nsis.FullName)"
$p = Start-Process $nsis.FullName -ArgumentList '/S' -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "NSIS install failed ($($p.ExitCode))" }
$exe = Find-App "$env:LOCALAPPDATA\Termalith"
if (-not $exe) { $exe = Find-App "$env:ProgramFiles\Termalith" }
if (-not $exe) { throw "installed app not found after the NSIS install" }
Write-Host "installed: $($exe.FullName)"
& "$PSScriptRoot\smoke-test.ps1" -Bin $exe.FullName
Write-Host "PASS: installers install and the app starts"
