# Native Windows desktop installer. No shell wrappers, dependency installation,
# daemon termination during upgrade, or overwriting of running executables.
[CmdletBinding()]
param(
    [string]$AppHome = $env:HERDR_APP_HOME,
    [string]$AppBin = $env:HERDR_APP_BIN,
    [string]$Binary,
    [string]$RuntimeBase,
    [string]$Profile = 'release',
    [string]$Target = 'x86_64-pc-windows-msvc',
    [int]$PanelWidth,
    [ValidateSet('native','nvim','')][string]$Tree = '',
    [switch]$Uninstall,
    [switch]$Force
)
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'herdr/windows.ps1')
Assert-WindowsX64
$repo = Split-Path -Parent $PSScriptRoot
if (-not $AppHome) { $AppHome = Join-Path $HOME '.config/herdr-app' }
$AppHome = (Resolve-Path -LiteralPath $AppHome).Path
if (-not $AppBin) { $AppBin = (Get-Command herdr-app.exe -ErrorAction Stop).Source }
$AppBin = (Resolve-Path -LiteralPath $AppBin).Path
if (-not $RuntimeBase) { $RuntimeBase = Join-Path $env:LOCALAPPDATA 'herdr-nvim/app' }
$RuntimeBase = [IO.Path]::GetFullPath($RuntimeBase)
$mods = @('herdr-nvim', 'herdr-nvim-header')
$owner = 'herdr-nvim.windows.v1'
$utf8 = New-Object Text.UTF8Encoding($false)
$bindingSchema = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'binding-schema.json') -Raw | ConvertFrom-Json
# This is a fixed binding contract, not the candidate's self-reported usage.
# Validate both installed and candidate metadata before staging or app calls.
function Test-BindingFields($Value, [string[]]$Fields) {
    if ($Value -isnot [Management.Automation.PSCustomObject]) { return $false }
    $actual = @($Value.PSObject.Properties.Name)
    if ($actual.Count -ne $Fields.Count) { return $false }
    foreach ($field in $Fields) { if ($actual -cnotcontains $field) { return $false } }
    return $true
}
function Test-BindingPath($Value) {
    return ($Value -is [string] -and $Value -cmatch '^(?:[A-Za-z]:[\\/]|[\\/]{2}[^\\/]+[\\/][^\\/]+)')
}
function Assert-CoreBinding($Core, [string]$Source) {
    $reason = "$Source has a legacy/unverifiable core binding (required schema v$($bindingSchema.version)); save/quit old nvims and verify exit before manual migration. Nothing stopped or replaced."
    if (-not (Test-BindingFields $Core @('version','hc_bin','selectors','config_path','config_fingerprint','app_home_identity'))) { throw $reason }
    if (($Core.version -isnot [int] -and $Core.version -isnot [long]) -or $Core.version -ne $bindingSchema.version) { throw $reason }
    if (-not (Test-BindingPath $Core.hc_bin) -or -not (Test-BindingPath $Core.config_path)) { throw $reason }
    if ($null -ne $Core.config_fingerprint -and ($Core.config_fingerprint -isnot [string] -or $Core.config_fingerprint -cnotmatch '^[0-9a-f]{16}$')) { throw $reason }
    $id = $Core.app_home_identity
    if (-not (Test-BindingFields $id @('volume_serial','directory_id'))) { throw $reason }
    if ($id.volume_serial -isnot [string] -or $id.volume_serial -cnotmatch '^[0-9a-f]{16}$' -or
        $id.directory_id -isnot [string] -or $id.directory_id -cnotmatch '^[0-9a-f]{32}$' -or $id.directory_id -ceq ('0' * 32)) { throw $reason }
    if (-not (Test-BindingFields $Core.selectors $bindingSchema.selectors)) { throw $reason }
    foreach ($name in $bindingSchema.selectors) {
        $value = $Core.selectors.$name
        if ($null -ne $value -and ($value -isnot [string] -or ($name -cne 'HC_SESSION' -and -not (Test-BindingPath $value)))) { throw $reason }
    }
    if ($null -eq $Core.selectors.XDG_RUNTIME_DIR -and $null -eq $Core.selectors.TMP -and $null -eq $Core.selectors.TEMP) { throw $reason }
    if ($null -eq $Core.selectors.XDG_STATE_HOME -and $null -eq $Core.selectors.HOME -and $null -eq $Core.selectors.USERPROFILE) { throw $reason }
}
function Write-Utf8([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Invoke-Checked([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') failed ($LASTEXITCODE)" }
}
function Read-Ownership([string]$Dir) {
    if (-not (Test-Path -LiteralPath $Dir)) { return $null }
    if ((Get-Item -LiteralPath $Dir -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Linked mod directory $Dir is not replaced; move the link away manually first"
    }
    $mark = Join-Path $Dir '.herdr-nvim'
    if (-not (Test-Path -LiteralPath $mark)) { throw "$Dir is not owned by this installer; move it away manually. Nothing replaced." }
    $value = Get-Content -LiteralPath $mark -Raw -Encoding UTF8 | ConvertFrom-Json
    if (-not [string]::Equals($value.owner, $owner, [StringComparison]::Ordinal)) { throw "Foreign mod at $Dir; nothing replaced" }
    return $value
}
function Read-BoundEntry($Mark) {
    $entry = [string]$Mark.entry
    $sidecar = [IO.Path]::ChangeExtension($entry, 'app.json')
    if (-not $Mark.PSObject.Properties['sidecar_sha256'] -or -not (Test-Path -LiteralPath $sidecar)) {
        throw 'Installed entry has no verifiable instance binding. Save/quit old nvims and migrate manually; nothing stopped or removed.'
    }
    if (-not [string]::Equals((Get-HerdrSha256 $sidecar), $Mark.sidecar_sha256, [StringComparison]::Ordinal)) { throw 'Installed sidecar differs from ownership binding; nothing stopped or removed' }
    $bound = Get-Content -LiteralPath $sidecar -Raw -Encoding UTF8 | ConvertFrom-Json
    if (-not $bound.PSObject.Properties['core']) { throw 'Installed entry has no core binding; nothing stopped or removed' }
    Assert-CoreBinding $bound.core 'Installed entry'
    # The native entry verifies its recorded directory identity before commands;
    # home-check compares this target to that record, not two current paths.
    & $entry app-home-check $AppHome | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw 'Installed entry belongs to a different or unverifiable AppHome/core instance. Use its original home or repair the restored copy manually; nothing stopped or removed.'
    }
    return $bound
}
# Scope all app CLI calls to the selected home, including preflight.
$names = @('HERDR_NVIM_HOST','HERDR_NVIM_PLUGIN_ROOT','HERDR_NVIM_SELF','HERDR_APP_HOME','HERDR_APP_BIN')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$lock = $null
$stage = $null
$swapped = @()
$backups = @{}
$preserveStage = $false
try {
    $env:HERDR_APP_HOME = $AppHome
    $env:HERDR_APP_BIN = $AppBin
    $env:HERDR_NVIM_HOST = 'app'
    $installed = Read-Ownership (Join-Path $AppHome 'mods/herdr-nvim')
    $bound = if ($installed) { Read-BoundEntry $installed } else { $null }
    if (-not $Uninstall) {
        if (-not $Binary) { $Binary = Build-HerdrNvim $repo $Profile $Target }
        $Binary = (Resolve-Path -LiteralPath $Binary).Path
        Assert-HerdrNvimBinary $Binary
        $coreJson = & $Binary app-core-binding
        if ($LASTEXITCODE -ne 0) { throw 'Cannot capture core binding; nothing replaced' }
        $coreText = ($coreJson -join "`n").Trim()
        if (-not $coreText.StartsWith('{')) { throw 'Candidate binary has a legacy/unverifiable core binding: expected a JSON object; nothing stopped or replaced' }
        try { $core = $coreText | ConvertFrom-Json -ErrorAction Stop }
        catch { throw 'Candidate binary has a legacy/unverifiable core binding: invalid JSON; nothing stopped or replaced' }
        Assert-CoreBinding $core 'Candidate binary'
        if ($bound -and -not [string]::Equals(($bound.core | ConvertTo-Json -Depth 8 -Compress), ($core | ConvertTo-Json -Depth 8 -Compress), [StringComparison]::Ordinal)) {
            throw 'Core binding/config changed. Save and quit the old instance, verify exit and migrate manually; installation will not abandon its daemons.'
        }
    }
    # Exclusive installer lock released on crash; never unlink a lock in use.
    $lock = [IO.File]::Open((Join-Path $AppHome '.herdr-nvim-install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    Invoke-Checked $AppBin @('status')
    foreach ($m in $mods) { $null = Read-Ownership (Join-Path $AppHome "mods/$m") }
    $keys = Join-Path $PSScriptRoot 'keys.windows.json'
    if ($Uninstall) {
        $installed = Read-Ownership (Join-Path $AppHome 'mods/herdr-nvim')
        if (-not $installed) { Write-Host 'Not installed'; return }
        $entry = [string]$installed.entry
        $stop = @('daemons','stop','--all')
        if ($Force) { $stop += '--force' }
        # Failure (unsaved/comments/hung/legacy) keeps mods and runtime intact.
        Invoke-Checked $entry $stop
        Invoke-Checked $entry @('app-setup','uninstall','--keys',$keys)
        foreach ($m in $mods) { Remove-Item -LiteralPath (Join-Path $AppHome "mods/$m") -Recurse -Force }
        Write-Host 'Uninstalled. Versioned runtimes retained deliberately; no running exe or old Lua was removed.'
        return
    }
    try { Invoke-Checked $Binary @('app-nvim-check') }
    catch { throw 'Neovim 0.10+ is required. Install it manually from https://neovim.io and put nvim.exe on PATH (or set [sidebar] nvim_bin). Nothing is downloaded automatically.' }
    New-Item -ItemType Directory -Force -Path $RuntimeBase | Out-Null
    # Unique immutable runtime for each install, also safe across app homes.
    $stamp = (Get-HerdrSha256 $Binary).Substring(0,16) + '-' + [guid]::NewGuid().ToString('N')
    $root = Join-Path $RuntimeBase $stamp
    $pending = Join-Path $RuntimeBase ('.new-' + $stamp)
    New-Item -ItemType Directory -Path (Join-Path $pending 'bin') -Force | Out-Null
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $pending 'bin/herdr-nvim.exe')
    foreach ($d in @('lua','plugin')) { Copy-Item -LiteralPath (Join-Path $repo $d) -Destination $pending -Recurse }
    Write-Utf8 (Join-Path $pending 'runtime-stamp') $stamp
    $entry = Join-Path $root 'bin/herdr-nvim.exe'
    Write-Utf8 (Join-Path $pending 'bin/herdr-nvim.app.json') (@{plugin_root=$root;app_home=$AppHome;app_bin=$AppBin;core=$core} | ConvertTo-Json -Depth 8 -Compress)
    Move-Item -LiteralPath $pending -Destination $root
    Assert-HerdrNvimBinary $entry
    $setup = @('--keys',$keys)
    if ($PanelWidth) { $setup += @('--panel-width',"$PanelWidth") }
    Invoke-Checked $entry (@('app-setup','check') + $setup)
    $modDir = Join-Path $AppHome 'mods'
    New-Item -ItemType Directory -Force -Path $modDir | Out-Null
    $stage = Join-Path $modDir ('.herdr-nvim-new-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage | Out-Null
    foreach ($m in $mods) {
        $new = Join-Path $stage $m
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot "mods/$m") -Destination $new -Recurse
        # All JS entry points use this directly executable runtime path.
        Write-Utf8 (Join-Path $new 'runtime.js') ('export const entry = ' + (ConvertTo-Json -InputObject $entry -Compress) + ';')
        Write-Utf8 (Join-Path $new '.herdr-nvim') (@{owner=$owner;entry=$entry;sidecar_sha256=(Get-HerdrSha256 ([IO.Path]::ChangeExtension($entry,'app.json')))} | ConvertTo-Json -Compress)
        Invoke-Checked $AppBin @('check',$new)
    }
    try {
        foreach ($m in $mods) {
            $live = Join-Path $modDir $m
            if (Test-Path -LiteralPath $live) {
                $backups[$m] = Join-Path $stage "$m.old"
                Move-Item -LiteralPath $live -Destination $backups[$m]
            }
            $swapped += $m
            Move-Item -LiteralPath (Join-Path $stage $m) -Destination $live
        }
        Invoke-Checked $entry (@('app-setup','install') + $setup)
        if ($Tree) { Invoke-Checked $entry @('app-config','tree',$Tree) }
    } catch {
        # Preserve backups if rollback itself fails (for example a sharing lock).
        $preserveStage = $true
        Write-Warning "Install failed; restoring previous mods. If restoration fails, backups remain at $stage"
        foreach ($m in $swapped) {
            $live = Join-Path $modDir $m
            if (Test-Path -LiteralPath $live) { Remove-Item -LiteralPath $live -Recurse -Force }
            if ($backups.ContainsKey($m)) { Move-Item -LiteralPath $backups[$m] -Destination $live }
        }
        $preserveStage = $false
        throw
    }
    Write-Host "Installed for $AppHome. Old runtimes/daemons are kept; save and quit stale tabs to load the new Lua. Windows key bindings are opt-in: see WINDOWS.md."
} finally {
    if (-not $preserveStage -and $stage -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
    if ($lock) { $lock.Dispose() }
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
