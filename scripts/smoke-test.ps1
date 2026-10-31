# Desktop smoke test (Windows): launch the built app, wait until the UI has
# opened a terminal (a shell child process exists), then stop the app and
# check that no shell is left behind.
#
# Usage: pwsh scripts/smoke-test.ps1 -Bin <path-to-app.exe>
param(
  [Parameter(Mandatory = $true)][string]$Bin,
  [int]$StartTimeout = 60
)
$ErrorActionPreference = 'Stop'

$shellName = Split-Path -Leaf $env:COMSPEC
$app = Start-Process -FilePath $Bin -PassThru
Write-Host "app pid $($app.Id); expecting child $shellName"

function Get-Shells {
  @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id)" |
      Where-Object { $_.Name -ieq $shellName })
}

try {
  $deadline = (Get-Date).AddSeconds($StartTimeout)
  while ((Get-Shells).Count -eq 0) {
    if ($app.HasExited) { throw "app exited during startup (code $($app.ExitCode))" }
    if ((Get-Date) -gt $deadline) { throw "no shell child after ${StartTimeout}s (frontend did not open a terminal)" }
    Start-Sleep -Milliseconds 500
  }
  Start-Sleep -Seconds 2
  $shells = Get-Shells
  Write-Host "shell children: $($shells.Count)"
  Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id)" |
    Format-Table ProcessId, Name, CommandLine -AutoSize | Out-String | Write-Host
  if ($shells.Count -ne 1) { throw "expected exactly one shell, found $($shells.Count)" }

  # Forced stop: checks the OS-level guarantee (closing the pseudo console
  # ends the attached shell) rather than the app's own cleanup.
  Stop-Process -Id $app.Id -Force
  $deadline = (Get-Date).AddSeconds(10)
  foreach ($s in $shells) {
    while (Get-Process -Id $s.ProcessId -ErrorAction SilentlyContinue) {
      if ((Get-Date) -gt $deadline) { throw "shell $($s.ProcessId) survived app termination" }
      Start-Sleep -Milliseconds 250
    }
  }
  Write-Host "PASS: terminal opened and shell cleaned up"
}
finally {
  if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
}
