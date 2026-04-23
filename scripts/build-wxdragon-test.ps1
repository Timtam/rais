[CmdletBinding()]
param(
    [switch]$Release,
    [string]$OutDir = "target\wxdragon-test"
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
Set-Location $RepoRoot

if (-not $env:LIBCLANG_PATH) {
    $CandidateLibclangDirs = @(
        "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\Llvm\x64\bin",
        "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin",
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin",
        "C:\Program Files\LLVM\bin"
    )

    foreach ($Candidate in $CandidateLibclangDirs) {
        if (Test-Path -LiteralPath (Join-Path $Candidate "libclang.dll")) {
            $env:LIBCLANG_PATH = $Candidate
            break
        }
    }
}

if (-not $env:LIBCLANG_PATH) {
    throw "LIBCLANG_PATH is not set and libclang.dll was not found in a known Visual Studio/LLVM path."
}

if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
    throw "Ninja is required for the wxDragon build. Install it with: winget install --id Ninja-build.Ninja -e"
}

$ProfileArgs = @()
$ProfileName = "debug"
if ($Release) {
    $ProfileArgs += "--release"
    $ProfileName = "release"
}

& cargo build -p rais-ui-wxdragon --features gui @ProfileArgs
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$SourceExe = Join-Path $RepoRoot "target\$ProfileName\rais-ui-wxdragon.exe"
if (-not (Test-Path -LiteralPath $SourceExe)) {
    throw "Expected wxDragon executable was not produced: $SourceExe"
}

if ([System.IO.Path]::IsPathRooted($OutDir)) {
    $ResolvedOutDir = $OutDir
} else {
    $ResolvedOutDir = Join-Path $RepoRoot $OutDir
}

New-Item -ItemType Directory -Force -Path $ResolvedOutDir | Out-Null

$TargetExe = Join-Path $ResolvedOutDir "RAIS-wxdragon-test.exe"
Copy-Item -LiteralPath $SourceExe -Destination $TargetExe -Force

$SourcePdb = [System.IO.Path]::ChangeExtension($SourceExe, ".pdb")
if (Test-Path -LiteralPath $SourcePdb) {
    Copy-Item -LiteralPath $SourcePdb -Destination (Join-Path $ResolvedOutDir "RAIS-wxdragon-test.pdb") -Force
}

Write-Host "wxDragon test executable: $TargetExe"
