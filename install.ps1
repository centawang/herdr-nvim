# Native Windows desktop entry, run from a clone. Matching release bundle by
# default; -Source uses an existing development toolchain, never installs one.
[CmdletBinding()]
param([switch]$Source, [switch]$Tui, [switch]$Uninstall, [switch]$Force,
      [string]$AppHome, [string]$AppBin, [string]$RuntimeBase,
      [string]$Version, [string]$Profile = 'release', [string]$Target = 'x86_64-pc-windows-msvc')
. (Join-Path $PSScriptRoot 'herdr/windows.ps1')
Assert-WindowsX64
if ($Tui) {
    if ($Uninstall) { throw 'Use herdr plugin uninstall chmarax.herdr-nvim for a registered TUI plugin' }
    & (Join-Path $PSScriptRoot 'herdr/install.ps1') -Source:$Source -Profile $Profile -Target $Target
    return
}
$params = @{Uninstall=$Uninstall;Force=$Force;Profile=$Profile;Target=$Target}
foreach ($name in @('AppHome','AppBin','RuntimeBase')) {
    $value = Get-Variable -Name $name -ValueOnly
    if ($value) { $params[$name] = $value }
}
if ($Source -or $Uninstall) { & (Join-Path $PSScriptRoot 'herdr-app/install.ps1') @params; return }
if (-not $Version) { $Version = Get-HerdrNvimVersion $PSScriptRoot }
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Version must be X.Y.Z' }
$work = Join-Path ([IO.Path]::GetTempPath()) ('hn-bundle-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $name = 'herdr-nvim-app-x86_64-pc-windows-msvc'
    $archive = Join-Path $work "$name.zip"
    Receive-CheckedAsset "https://github.com/ChmaraX/herdr-nvim/releases/download/v$Version/$name.zip" $archive
    # Reject zip entries that escape the staging directory, before extraction.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        foreach ($entry in $zip.Entries) {
            $path = [IO.Path]::GetFullPath((Join-Path $work $entry.FullName))
            if (-not $path.StartsWith($work + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Archive path escapes staging directory' }
        }
    } finally { $zip.Dispose() }
    [IO.Compression.ZipFile]::ExtractToDirectory($archive, $work)
    $bundle = Join-Path $work $name
    $params.Binary = Join-Path $bundle 'bin/herdr-nvim.exe'
    & (Join-Path $bundle 'herdr-app/install.ps1') @params
} finally { Remove-Item -LiteralPath $work -Recurse -Force }
