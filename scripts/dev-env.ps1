# Loads the MSVC x64 environment (vcvars64) into the current PowerShell session,
# then runs the given cargo command. Usage: .\scripts\dev-env.ps1 'build' | 'test' | 'run -- ls'
$ErrorActionPreference = 'Stop'
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$install = $null
if (Test-Path $vswhere) {
    $install = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $install) {
        $install = & $vswhere -prerelease -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    }
}
if (-not $install) { throw 'No Visual Studio installation with the C++ toolset found' }
$vcvars = Join-Path $install 'VC\Auxiliary\Build\vcvars64.bat'
$out = & "$env:ComSpec" /c "`"$vcvars`" >nul 2>&1 && set"
foreach ($line in $out) {
    if ($line -match '^(.+?)=(.*)$') {
        [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process')
    }
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Invoke-Expression "cargo $args"
exit $LASTEXITCODE
