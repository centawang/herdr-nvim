[CmdletBinding()]
param([Parameter(Mandatory)][string]$Binary, [string]$Target = 'x86_64-pc-windows-msvc', [string]$Out = '.')
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'herdr/windows.ps1')
$repo = Split-Path -Parent $PSScriptRoot
Assert-HerdrNvimBinary $Binary
$Binary = (Resolve-Path -LiteralPath $Binary).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ('hn-pack-' + [guid]::NewGuid().ToString('N'))
$name = "herdr-nvim-app-$Target"
$bundle = Join-Path $work $name
try {
    New-Item -ItemType Directory -Path (Join-Path $bundle 'bin'),(Join-Path $bundle 'herdr-app'),(Join-Path $bundle 'herdr') -Force | Out-Null
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $bundle 'bin/herdr-nvim.exe')
    foreach ($d in @('lua','plugin')) { Copy-Item -LiteralPath (Join-Path $repo $d) -Destination $bundle -Recurse }
    foreach ($f in @('install.ps1','keys.windows.json','binding-schema.json')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $f) -Destination (Join-Path $bundle 'herdr-app') }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'mods') -Destination (Join-Path $bundle 'herdr-app/mods') -Recurse
    Copy-Item -LiteralPath (Join-Path $repo 'herdr/windows.ps1') -Destination (Join-Path $bundle 'herdr')
    foreach ($f in @('herdr-plugin.toml','WINDOWS.md')) { Copy-Item -LiteralPath (Join-Path $repo $f) -Destination $bundle }
    $asset = Join-Path $Out "$name.zip"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    if (Test-Path -LiteralPath $asset) { Remove-Item -LiteralPath $asset -Force }
    [IO.Compression.ZipFile]::CreateFromDirectory($bundle, $asset, [IO.Compression.CompressionLevel]::Optimal, $true)
    $hash = Get-HerdrSha256 $asset
    [IO.File]::WriteAllText("$asset.sha256", "$hash  $name.zip`n", (New-Object Text.UTF8Encoding($false)))
} finally { if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force } }
