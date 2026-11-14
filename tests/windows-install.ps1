# Parse-only runs with existing PowerShell, without Rust/Neovim or user IO.
# Full smoke requires a real binary; -FixtureOnly tests transactions without Rust.
# Every install is into
# a throwaway home with native fake app/nvim; no real app/UI commands are sent.
[CmdletBinding()]
param([switch]$ParseOnly, [switch]$FixtureOnly, [string]$Binary)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$scripts = @('install.ps1','herdr/windows.ps1','herdr/install.ps1','herdr-app/install.ps1','herdr-app/bundle.ps1','tests/windows-install.ps1','tests/windows-process.ps1','tests/support/compile-install-fixture.ps1')
foreach ($script in $scripts) {
    $tokens = $null; $errors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile((Join-Path $repo $script), [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw "$script : $errors" }
}
. (Join-Path $repo 'herdr/windows.ps1')
$cargo = Get-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Cargo package version missing' }
if ((Get-HerdrNvimVersion $repo) -ne $Matches[1]) { throw 'manifest and binary package versions differ' }
Write-Host 'PowerShell parse/version checks: ok'
if ($ParseOnly) { return }
if (-not $Binary -and -not $FixtureOnly) { throw '-Binary is required for full smoke (or -FixtureOnly for transaction tests without Rust)' }
if ($Binary) { Assert-HerdrNvimBinary (Resolve-Path -LiteralPath $Binary).Path }
$work = Join-Path ([IO.Path]::GetTempPath()) ('hn-install-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
function Check([bool]$Pass, [string]$Why) { if (-not $Pass) { throw $Why } }
function Must-Fail([scriptblock]$Run) { $failed = $false; try { & $Run } catch { $failed = $true }; Check $failed 'expected safe failure' }
try {
    $fixture = Join-Path $work 'fixture.exe'
    & powershell.exe -NoProfile -NonInteractive -File (Join-Path $PSScriptRoot 'support/compile-install-fixture.ps1') -Output $fixture
    if ($LASTEXITCODE -ne 0) { throw 'test fixture compilation failed' }
    # Exact download integrity path, no network: supply private fixture bytes.
    $script:FakeChecksum = Get-HerdrSha256 $fixture
    function Invoke-WebRequest([switch]$UseBasicParsing, [string]$Uri, [string]$OutFile) {
        if ($Uri.EndsWith('.sha256')) { [IO.File]::WriteAllText($OutFile, $script:FakeChecksum) }
        else { Copy-Item -LiteralPath $fixture -Destination $OutFile }
    }
    $download = Join-Path $work 'download.exe'
    Receive-CheckedAsset 'https://fixture.invalid/binary.exe' $download
    $dest = Join-Path $work 'installed/herdr-nvim.exe'
    Set-HerdrNvimBinary $download $dest
    $before = Get-HerdrSha256 $dest
    $script:FakeChecksum = '0' * 64
    Must-Fail { Receive-CheckedAsset 'https://fixture.invalid/binary.exe' $download }
    [IO.File]::WriteAllText($download, 'not executable')
    Must-Fail { Set-HerdrNvimBinary $download $dest }
    Check ((Get-HerdrSha256 $dest) -eq $before) 'invalid candidate replaced binary'
    $out = Join-Path $work 'bundle'
    & (Join-Path $repo 'herdr-app/bundle.ps1') -Binary $fixture -Out $out
    $zip = Join-Path $out 'herdr-nvim-app-x86_64-pc-windows-msvc.zip'
    Check (Test-Path -LiteralPath $zip) 'bundle missing'
    Check ((Get-Content -LiteralPath "$zip.sha256" -Raw).StartsWith((Get-HerdrSha256 $zip))) 'bundle checksum mismatch'
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        # .NET Framework ZipFile uses backslashes for Windows-created ZIPs.
        $schemaEntries = @($archive.Entries | Where-Object { $_.FullName.Replace('\','/') -ceq 'herdr-nvim-app-x86_64-pc-windows-msvc/herdr-app/binding-schema.json' })
        Check ($schemaEntries.Count -eq 1) 'bundle omitted/duplicated binding schema'
        $schemaEntry = $schemaEntries[0]
        Write-Host "Bundle schema entry: $($schemaEntry.FullName)"
        $reader = New-Object IO.StreamReader($schemaEntry.Open())
        try { $bundledSchema = $reader.ReadToEnd() } finally { $reader.Dispose() }
        Check ($bundledSchema -ceq (Get-Content -LiteralPath (Join-Path $repo 'herdr-app/binding-schema.json') -Raw)) 'bundled schema differs from Rust/installer source'
    } finally { $archive.Dispose() }
    $homeDir = Join-Path $work ('app home ' + [char]0x7a7a + ' ' + [char]0x683c)
    $runtime = Join-Path $work 'runtime'
    New-Item -ItemType Directory -Path $homeDir | Out-Null
    $keys = '{"global":{"ctrl-e":"someone-else.action"}}'
    [IO.File]::WriteAllText((Join-Path $homeDir 'keys.json'), $keys)
    [IO.File]::WriteAllText((Join-Path $homeDir 'layout.json'), '{"keep":"layout sentinel"}')
    $params = @{AppHome=$homeDir;AppBin=$fixture;Binary=$fixture;RuntimeBase=$runtime}
    $installer = Join-Path $repo 'herdr-app/install.ps1'
    # Locally compiled trusted native candidate with accurate old v2 metadata.
    # A usage/exit-2 probe is NOT evidence that its binding schema is supported.
    $oldSelectors = [ordered]@{}
    foreach ($n in @('HC_SESSION','HC_SOCKET','HOME','USERPROFILE','XDG_CONFIG_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR','TEMP','TMP','APPDATA','LOCALAPPDATA')) {
        $oldSelectors[$n] = [Environment]::GetEnvironmentVariable($n)
    }
    $oldCore = [ordered]@{version=2;hc_bin=$fixture;selectors=$oldSelectors;config_path=(Join-Path $work 'missing-core.toml');config_fingerprint=$null}
    $env:HERDR_APP_HOME = $homeDir
    $valid = (& $fixture app-core-binding | ConvertFrom-Json)
    $cases = [ordered]@{'old-v2'=($oldCore | ConvertTo-Json -Depth 8 -Compress)}
    $oldCore.version = 1
    $cases['old-v1'] = $oldCore | ConvertTo-Json -Depth 8 -Compress
    foreach ($field in @('version','hc_bin','selectors','config_path','config_fingerprint','app_home_identity')) {
        $bad = $valid | ConvertTo-Json -Depth 8 | ConvertFrom-Json
        $bad.PSObject.Properties.Remove($field)
        $cases["missing-$field"] = $bad | ConvertTo-Json -Depth 8 -Compress
    }
    foreach ($kind in @('unknown-version','string-version','bad-id','zero-id','missing-volume','unknown-field','bad-selectors','missing-selector','relative-path')) {
        $bad = $valid | ConvertTo-Json -Depth 8 | ConvertFrom-Json
        switch ($kind) {
            'unknown-version' { $bad.version = $valid.version + 1 }
            'string-version' { $bad.version = [string]$valid.version }
            'bad-id' { $bad.app_home_identity.directory_id = 'not-a-file-id' }
            'zero-id' { $bad.app_home_identity.directory_id = '0' * 32 }
            'missing-volume' { $bad.app_home_identity.PSObject.Properties.Remove('volume_serial') }
            'unknown-field' { $bad | Add-Member NoteProperty unexpected 'not supported' }
            'bad-selectors' { $bad.selectors = @() }
            'missing-selector' { $bad.selectors.PSObject.Properties.Remove('HC_SESSION') }
            'relative-path' { $bad.hc_bin = 'relative.exe' }
        }
        $cases[$kind] = $bad | ConvertTo-Json -Depth 8 -Compress
    }
    $cases['null'] = 'null'
    $cases['array'] = '[' + ($valid | ConvertTo-Json -Depth 8 -Compress) + ']'
    $cases['invalid-json'] = '{bad JSON'
    function Tree-Snapshot([string]$Dir) {
        if (-not (Test-Path -LiteralPath $Dir)) { return '<absent>' }
        return ((Get-ChildItem -LiteralPath $Dir -Force -Recurse | Sort-Object FullName | ForEach-Object {
            if ($_.PSIsContainer) { 'D:' + $_.FullName } else { $_.FullName + ':' + (Get-HerdrSha256 $_.FullName) }
        }) -join "`n")
    }
    function Candidate-Rejections([string]$Phase) {
        $beforeHome = Tree-Snapshot $homeDir
        $beforeRuntime = Tree-Snapshot $runtime
        foreach ($case in $cases.Keys) {
            $env:HN_BINDING_JSON = Join-Path $work 'candidate.json'
            $env:HN_COMMAND_LOG = Join-Path $work 'candidate-commands.log'
            Remove-Item -LiteralPath $env:HN_COMMAND_LOG -ErrorAction SilentlyContinue
            [IO.File]::WriteAllText($env:HN_BINDING_JSON, $cases[$case])
            $reason = ''
            try { & $installer @params } catch { $reason = $_.Exception.Message }
            Check ($reason -match 'Candidate binary has a legacy/unverifiable core binding') "$Phase/$case was not rejected by candidate schema guard: $reason"
            Check ((Tree-Snapshot $homeDir) -ceq $beforeHome) "$Phase/$case changed home/mods/keys/layout/lock"
            Check ((Tree-Snapshot $runtime) -ceq $beforeRuntime) "$Phase/$case staged or changed runtime"
            $calls = Get-Content $env:HN_COMMAND_LOG -Raw
            Check ($calls -notmatch '(?m) (?:status|app-nvim-check|app-setup|daemons|check|layout|keys)(?: |$)') "$Phase/$case reached app/setup/stop/layout/mod check"
            Write-Host "PASS $Phase candidate $case rejected before staging/actions"
        }
        Remove-Item Env:HN_BINDING_JSON,Env:HN_COMMAND_LOG
    }
    Candidate-Rejections 'fresh'
    Check (-not (Test-Path $runtime)) 'invalid candidate reached runtime staging'
    Check (-not (Test-Path (Join-Path $homeDir 'mods'))) 'invalid candidate staged mods'
    & $installer @params
    $mark = Join-Path $homeDir 'mods/herdr-nvim/.herdr-nvim'
    $first = (Get-Content $mark -Raw -Encoding UTF8 | ConvertFrom-Json).entry
    Check (Test-Path -LiteralPath $first) 'native versioned entry missing'
    $sidecarPath = [IO.Path]::ChangeExtension($first,'app.json')
    Check (Test-Path -LiteralPath $sidecarPath) 'sidecar missing'
    Check ((Get-Content -LiteralPath $sidecarPath -Raw -Encoding UTF8 | ConvertFrom-Json).app_home -eq $homeDir) 'Unicode/space app home lost in sidecar'
    Check ((Get-Content (Join-Path $homeDir 'keys.json') -Raw) -eq $keys) 'user keys overwritten'
    # A running exe cannot be rewritten: hold its file open without delete sharing.
    $held = [IO.File]::Open($first, 'Open', 'Read', 'Read')
    try { & $installer @params } finally { $held.Dispose() }
    $second = (Get-Content $mark -Raw -Encoding UTF8 | ConvertFrom-Json).entry
    Check ($first -ne $second -and (Test-Path -LiteralPath $first)) 'upgrade rewrote/deleted old runtime'
    Candidate-Rejections 'upgrade'
    $env:HN_FAIL_CHECK = '1'
    Must-Fail { & $installer @params }
    Remove-Item Env:HN_FAIL_CHECK
    Check ((Get-Content $mark -Raw -Encoding UTF8 | ConvertFrom-Json).entry -eq $second) 'check failure replaced live mod'
    $env:HN_FAIL_SETUP = '1'
    Must-Fail { & $installer @params }
    Remove-Item Env:HN_FAIL_SETUP
    Check ((Get-Content $mark -Raw -Encoding UTF8 | ConvertFrom-Json).entry -eq $second) 'setup failure did not restore mods'
    # Restoring A's owned mods into B must not run A's bound entry at all.
    $other = Join-Path $work 'restored-B'
    New-Item -ItemType Directory -Path $other | Out-Null
    Copy-Item -LiteralPath (Join-Path $homeDir 'mods') -Destination $other -Recurse
    $env:HN_COMMAND_LOG = Join-Path $work 'commands.log'
    Must-Fail { & $installer -AppHome $other -AppBin $fixture -Uninstall }
    $calls = if (Test-Path $env:HN_COMMAND_LOG) { Get-Content $env:HN_COMMAND_LOG -Raw } else { '' }
    Check ($calls -notmatch 'daemons|app-setup') 'mismatched uninstall invoked destructive entry'
    Check (Test-Path (Join-Path $other 'mods/herdr-nvim/.herdr-nvim')) 'mismatched uninstall deleted B mod'
    Check ((Get-Content $mark -Raw -Encoding UTF8 | ConvertFrom-Json).entry -eq $second) 'mismatched uninstall changed A mod'
    Remove-Item Env:HN_COMMAND_LOG
    $env:HN_DIRTY = '1'
    Must-Fail { & $installer @params -Uninstall }
    Remove-Item Env:HN_DIRTY
    Check (Test-Path -LiteralPath $mark) 'unsaved uninstall removed mod'
    & $installer @params -Uninstall
    Check (-not (Test-Path -LiteralPath $mark)) 'owned mod was not uninstalled'
    Check (Test-Path -LiteralPath $first) 'uninstall deleted potentially active runtime'
    New-Item -ItemType Directory -Path (Join-Path $homeDir 'mods/herdr-nvim') | Out-Null
    Must-Fail { & $installer @params }
    if ($FixtureOnly) {
        Write-Host 'Installer transaction checks: ok (stand-in executables only; real Rust entry NOT tested)'
        return
    }
    # Unlike app-config (valid for either host), this protocol suite requires
    # the sidecar: app-only IO, its image cache and real nvim -> SELF re-entry.
    & node (Join-Path $repo 'herdr-app/e2e/native-protocol.mjs') (Resolve-Path -LiteralPath $Binary).Path
    Check ($LASTEXITCODE -eq 0) 'native sidecar protocol failed'
    Write-Host 'Native installer transaction/protocol: ok (private app stand-in, not UI verification)'
} finally {
    Remove-Item Env:HN_FAIL_CHECK,Env:HN_FAIL_SETUP,Env:HN_DIRTY,Env:HN_COMMAND_LOG,Env:HN_BINDING_JSON,Env:HERDR_NVIM_HC -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $work -Recurse -Force
}
