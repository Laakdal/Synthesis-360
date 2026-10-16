<#
.SYNOPSIS
  Builds the CAD360 Windows distribution: dist\CAD360-<version>-win64.zip (+ .msi when WiX is
  installed) with licence notices, a sample design and SHA-256 checksums.

.EXAMPLE
  pwsh -File packaging\windows\make-dist.ps1                 # build + package
  pwsh -File packaging\windows\make-dist.ps1 -SkipBuild      # package the existing release exe
  pwsh -File packaging\windows\smoke-test.ps1                # then test the zip standalone

.NOTES
  See docs/build-and-release.md for prerequisites, timings and sizes.
#>
param(
    [string]$Version,
    [switch]$SkipBuild,
    [string]$SourceUrl = 'https://github.com/Laakdal/Synthesis-360',
    [string]$TargetDir = 'C:\c360t',
    [string]$CMakeBin = 'C:\tools\cmake-3.31.8-windows-x86_64\bin',
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $OutDir) { $OutDir = Join-Path $Root 'dist' }
if (-not $Version) {
    $m = Select-String -Path (Join-Path $Root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    $Version = $m.Matches[0].Groups[1].Value
}
$Name = "CAD360-$Version-win64"
$Exe = Join-Path $TargetDir 'release\cad360.exe'
$Occt = Join-Path $TargetDir 'OCCT'

function Step([string]$text) { Write-Host "==> $text" -ForegroundColor Cyan }

function Find-VsTool([string]$relative) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $null }
    $vs = & $vswhere -latest -products * -property installationPath
    if (-not $vs) { return $null }
    Get-ChildItem -Path (Join-Path $vs $relative) -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
}

# DLLs every supported Windows (10/11 x64) provides; anything else must ship with the exe.
$SystemDlls = @(
    'kernel32', 'kernelbase', 'ntdll', 'user32', 'gdi32', 'advapi32', 'shell32', 'ole32', 'oleaut32',
    'comctl32', 'comdlg32', 'shlwapi', 'ws2_32', 'bcrypt', 'bcryptprimitives', 'crypt32', 'secur32',
    'dwmapi', 'uxtheme', 'imm32', 'winmm', 'version', 'userenv', 'propsys', 'opengl32', 'd3d12',
    'dxgi', 'd3dcompiler_47', 'dbghelp', 'setupapi', 'cfgmgr32', 'hid', 'ntdll', 'powrprof', 'winspool',
    'msimg32', 'usp10', 'gdiplus', 'iphlpapi', 'netapi32', 'mpr', 'rpcrt4', 'combase', 'shcore', 'uiautomationcore'
)

function Get-Dependents([string]$path) {
    $dumpbin = Find-VsTool 'VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe'
    if (-not $dumpbin) { Write-Warning 'dumpbin.exe not found (Visual Studio C++ tools); skipping the dependency check'; return @() }
    & $dumpbin.FullName /nologo /dependents $path | ForEach-Object { $_.Trim() } | Where-Object { $_ -match '^[\w\-\.]+\.dll$' }
}

function Test-SystemDll([string]$dll) {
    $base = [IO.Path]::GetFileNameWithoutExtension($dll).ToLowerInvariant()
    ($base -like 'api-ms-win-*') -or ($base -like 'ext-ms-*') -or ($SystemDlls -contains $base)
}

# ------------------------------------------------------------------------------------- build
if (-not $SkipBuild) {
    Step "cargo build --release -p cad-app"
    if (Test-Path $CMakeBin) { $env:PATH = "$CMakeBin;$env:PATH" }
    $t = Measure-Command { Push-Location $Root; try { cargo build --release -p cad-app; if ($LASTEXITCODE) { throw "cargo build failed ($LASTEXITCODE)" } } finally { Pop-Location } }
    Write-Host ("    build time: {0:n1} s" -f $t.TotalSeconds)
}
if (-not (Test-Path $Exe)) { throw "Release executable not found: $Exe (run without -SkipBuild)" }

# ------------------------------------------------------------------------------------- stage
$Stage = Join-Path $OutDir $Name
Step "Staging $Stage"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force $Stage, (Join-Path $Stage 'samples'), (Join-Path $Stage 'THIRD-PARTY-NOTICES') | Out-Null
Copy-Item $Exe $Stage

# runtime dependencies: anything beyond Windows system DLLs (the VC++ runtime) is copied
# app-local from Visual Studio's redistributable folder, as Microsoft permits.
$deps = @(Get-Dependents $Exe)
$extra = @($deps | Where-Object { -not (Test-SystemDll $_) })
$runtimeNote = 'No other runtime needs to be installed.'
if ($extra.Count) {
    $crt = Find-VsTool 'VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT'
    foreach ($dll in $extra) {
        $src = if ($crt) { Join-Path $crt.FullName $dll }
        if ($src -and (Test-Path $src)) {
            Copy-Item $src $Stage
            Write-Host "    bundled $dll (Visual C++ runtime, app-local)"
        } else {
            Write-Warning "cad360.exe depends on $dll, which is neither a system DLL nor in the VC++ redist folder"
        }
    }
    $runtimeNote = 'The Visual C++ runtime DLLs it needs are included next to cad360.exe; no installer is required.'
}
Write-Host ("    dependents: " + ($deps -join ', '))

$occtVersion = (Select-String -Path (Join-Path $Occt 'include\Standard_Version.hxx') -Pattern 'define OCC_VERSION_COMPLETE\s+"([^"]+)"').Matches[0].Groups[1].Value
function Expand([string]$text) {
    $text.Replace('@VERSION@', $Version).Replace('@SOURCE_URL@', $SourceUrl).Replace('@OCCT_VERSION@', $occtVersion).Replace('@RUNTIME_NOTE@', $runtimeNote)
}
$files = Join-Path $PSScriptRoot 'dist-files'
Set-Content -Encoding utf8 -Path (Join-Path $Stage 'README.txt') -Value (Expand (Get-Content -Raw (Join-Path $files 'README.txt')))
Set-Content -Encoding utf8 -Path (Join-Path $Stage 'LICENSE.txt') -Value (Expand (Get-Content -Raw (Join-Path $files 'LICENSE.txt')))
$tp = Join-Path $Stage 'THIRD-PARTY-NOTICES'
Set-Content -Encoding utf8 -Path (Join-Path $tp 'OpenCASCADE-NOTICE.txt') -Value (Expand (Get-Content -Raw (Join-Path $files 'OpenCASCADE-NOTICE.txt')))
Copy-Item (Join-Path $Occt 'LICENSE_LGPL_21.txt') (Join-Path $tp 'OpenCASCADE-LGPL-2.1.txt')
Copy-Item (Join-Path $Occt 'OCCT_LGPL_EXCEPTION.txt') (Join-Path $tp 'OpenCASCADE-LGPL-EXCEPTION.txt')

# Rust crates compiled into the exe (normal dependencies for this target), with their licences
Push-Location $Root
try {
    $crates = cargo tree -p cad-app -e normal --target x86_64-pc-windows-msvc --prefix none --format '{p}|{l}' 2>$null |
        ForEach-Object { $_ -replace ' \(\*\)$', '' -replace ' \(proc-macro\)', '' } | Sort-Object -Unique
} finally { Pop-Location }
$lines = @('Rust crates compiled into cad360.exe', '====================================', '',
    'Name and version, then the licence declared by each crate. Full licence texts are published',
    'with each crate on https://crates.io.', '')
$lines += $crates | Where-Object { $_ -notmatch '^(cad-|opencascade)' } | ForEach-Object {
    $p = $_.Split('|'); '{0,-45} {1}' -f $p[0].Replace(' v', ' '), $(if ($p[1]) { $p[1] } else { '(see crate)' })
}
Set-Content -Encoding utf8 -Path (Join-Path $tp 'rust-crates.txt') -Value $lines
Write-Host "    $($crates.Count) crates listed"

# ------------------------------------------------------------------------------------- sample
Step 'Creating the sample design with the packaged exe'
$work = Join-Path ([IO.Path]::GetTempPath()) ("cad360-dist-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $work | Out-Null
Copy-Item (Join-Path $PSScriptRoot 'sample-design.json') $work
$log = Join-Path $work 'sample.log'
$p = Start-Process -FilePath (Join-Path $Stage 'cad360.exe') -WorkingDirectory $Stage -PassThru `
    -ArgumentList @('--scenario', (Join-Path $work 'sample-design.json'), '--screenshot-dir', (Join-Path $work 'shots')) `
    -RedirectStandardError $log -RedirectStandardOutput (Join-Path $work 'sample.out')
if (-not $p.WaitForExit(180000)) { $p.Kill(); throw 'Sample scenario timed out' }
$samplePath = Join-Path $Stage 'samples\mounting-plate.c360'
if (-not (Test-Path $samplePath)) { Get-Content $log | Write-Host; throw 'The sample design was not saved' }
if (Select-String -Path $log -Pattern '\[(Error|Warning)\]' -Quiet) { Write-Warning "Sample scenario reported problems:"; Select-String -Path $log -Pattern '\[(Error|Warning)\]' | ForEach-Object { Write-Warning $_.Line } }
Copy-Item (Join-Path $work 'shots\sample-mounting-plate.png') (Join-Path $OutDir "$Name-sample.png") -ErrorAction SilentlyContinue
Write-Host ("    samples\mounting-plate.c360 ({0:n0} bytes)" -f (Get-Item $samplePath).Length)

# ------------------------------------------------------------------------------------- checksums + zip
Step 'Checksums and zip'
$sums = Get-ChildItem -Recurse -File $Stage | Sort-Object FullName | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLowerInvariant(), $_.FullName.Substring($Stage.Length + 1).Replace('\', '/')
}
Set-Content -Encoding ascii -Path (Join-Path $Stage 'SHA256SUMS.txt') -Value $sums
$zip = Join-Path $OutDir "$Name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $Stage -DestinationPath $zip -CompressionLevel Optimal
$artifacts = @($zip)

# ------------------------------------------------------------------------------------- msi
$wix = Get-Command wix -ErrorAction SilentlyContinue
if ($wix) {
    Step "MSI (WiX $(& wix --version))"
    $msi = Join-Path $OutDir "$Name.msi"
    & wix build (Join-Path $PSScriptRoot 'cad360.wxs') -arch x64 -d "Version=$Version" -d "StageDir=$Stage" -d "IconPath=$(Join-Path $Root 'crates\cad-app\assets\cad360.ico')" -o $msi
    if ($LASTEXITCODE) { throw "wix build failed ($LASTEXITCODE)" }
    $artifacts += $msi
} else {
    Write-Host '    WiX not installed: MSI skipped (dotnet tool install --global wix; see docs/build-and-release.md)'
}

$outer = $artifacts | ForEach-Object { '{0}  {1}' -f (Get-FileHash -Algorithm SHA256 $_).Hash.ToLowerInvariant(), (Split-Path -Leaf $_) }
Set-Content -Encoding ascii -Path (Join-Path $OutDir 'SHA256SUMS.txt') -Value $outer
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue

Step 'Done'
Write-Host ("    cad360.exe  {0:n1} MB" -f ((Get-Item $Exe).Length / 1MB))
foreach ($a in $artifacts) { Write-Host ("    {0}  {1:n1} MB" -f (Split-Path -Leaf $a), ((Get-Item $a).Length / 1MB)) }
$outer | ForEach-Object { Write-Host "    $_" }
