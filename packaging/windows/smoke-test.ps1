<#
.SYNOPSIS
  Smoke-tests the packaged zip standalone: extracts it to a fresh temp folder, checks the exe's
  DLL imports, runs a scripted scenario (sketch, extrude, STEP export, open the bundled sample,
  screenshots) and verifies the outputs.

.EXAMPLE
  pwsh -File packaging\windows\smoke-test.ps1                       # newest dist\*.zip
  pwsh -File packaging\windows\smoke-test.ps1 -Zip dist\CAD360-0.1.0-win64.zip -Keep
#>
param([string]$Zip, [switch]$Keep, [int]$TimeoutSec = 180)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Zip) { $Zip = (Get-ChildItem (Join-Path $Root 'dist\CAD360-*-win64.zip') | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName }
if (-not $Zip -or -not (Test-Path $Zip)) { throw 'No distribution zip found; run make-dist.ps1 first' }

$failures = @()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "  PASS  $what" -ForegroundColor Green } else { Write-Host "  FAIL  $what" -ForegroundColor Red; $script:failures += $what }
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("cad360-smoke-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
Write-Host "Extracting $(Split-Path -Leaf $Zip) to $tmp"
Expand-Archive -Path $Zip -DestinationPath $tmp
$app = (Get-ChildItem -Directory $tmp | Select-Object -First 1).FullName
$exe = Join-Path $app 'cad360.exe'
Check (Test-Path $exe) 'cad360.exe extracted'

# checksums shipped in the folder match
$bad = Get-Content (Join-Path $app 'SHA256SUMS.txt') | ForEach-Object {
    $h, $f = $_ -split '  ', 2
    if ((Get-FileHash -Algorithm SHA256 (Join-Path $app $f)).Hash.ToLowerInvariant() -ne $h) { $f }
}
Check (-not $bad) 'SHA256SUMS.txt matches every file'

# imports: only Windows system DLLs, or DLLs shipped next to the exe
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$dumpbin = if (Test-Path $vswhere) { Get-ChildItem (Join-Path (& $vswhere -latest -products * -property installationPath) 'VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe') -ErrorAction SilentlyContinue | Select-Object -First 1 }
if ($dumpbin) {
    $deps = & $dumpbin.FullName /nologo /dependents $exe | ForEach-Object { $_.Trim() } | Where-Object { $_ -match '^[\w\-\.]+\.dll$' }
    Write-Host "  imports: $($deps -join ', ')"
    $missing = $deps | Where-Object {
        -not (Test-Path (Join-Path $app $_)) -and -not (Test-Path (Join-Path $env:SystemRoot "System32\$_")) -and ($_ -notlike 'api-ms-win-*')
    }
    Check (-not $missing) "every imported DLL is a system DLL or shipped ($($missing -join ', '))"
} else {
    Write-Warning 'dumpbin not found; import check skipped'
}

# run the scenario from the extracted folder (relative paths resolve there)
Copy-Item (Join-Path $PSScriptRoot 'smoke.json') $tmp
$shots = Join-Path $tmp 'shots'
$log = Join-Path $tmp 'smoke.log'
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $exe -WorkingDirectory $app -PassThru -ArgumentList @('--scenario', (Join-Path $tmp 'smoke.json'), '--screenshot-dir', $shots) `
    -RedirectStandardError $log -RedirectStandardOutput (Join-Path $tmp 'smoke.out')
$exited = $p.WaitForExit($TimeoutSec * 1000)
if (-not $exited) { $p.Kill() }
$sw.Stop()
Check $exited "scenario finished within $TimeoutSec s ($([int]$sw.Elapsed.TotalSeconds) s)"
Check ($exited -and $p.ExitCode -eq 0) "exit code 0 ($($p.ExitCode))"
foreach ($s in '01_startup', '02_extruded_box', '03_sample_design') {
    $f = Join-Path $shots "$s.png"
    Check ((Test-Path $f) -and (Get-Item $f).Length -gt 10000) "screenshot $s.png"
}
$step = Join-Path $app 'smoke-box.step'
Check ((Test-Path $step) -and (Select-String -Path $step -Pattern 'MANIFOLD_SOLID_BREP' -Quiet)) 'STEP export contains a solid'
$problems = Select-String -Path $log -Pattern '\[(Error|Warning)\]|could not|failed' | ForEach-Object { $_.Line }
Check (-not $problems) "no errors in the scenario log $($problems -join ' | ')"

Write-Host "Log: $log"
Write-Host "Screenshots: $shots"
if ($failures.Count) {
    Write-Host "$($failures.Count) check(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host 'Smoke test passed' -ForegroundColor Green
if (-not $Keep) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
