# TUI manifest hook: matching release first, never require Git Bash/WSL.
[CmdletBinding()]
param([switch]$Source = ($env:HERDR_NVIM_BUILD_FROM_SOURCE -eq '1'), [string]$Profile = 'release', [string]$Target = 'x86_64-pc-windows-msvc')
. (Join-Path $PSScriptRoot 'windows.ps1')
Assert-WindowsX64
$root = Split-Path -Parent $PSScriptRoot
$temp = Join-Path ([IO.Path]::GetTempPath()) ('hn-install-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
try {
    if ($Source) { $binary = Build-HerdrNvim $root $Profile $Target }
    else {
        $version = Get-HerdrNvimVersion $root
        $binary = Join-Path $temp 'herdr-nvim.exe'
        Receive-CheckedAsset "https://github.com/ChmaraX/herdr-nvim/releases/download/v$version/herdr-nvim-x86_64-pc-windows-msvc.exe" $binary
    }
    Set-HerdrNvimBinary $binary (Join-Path $root 'bin/herdr-nvim.exe')
} finally { Remove-Item -LiteralPath $temp -Recurse -Force }
