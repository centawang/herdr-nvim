# Shared native Windows build/download/probe helpers. Dot-source; no IO on load.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-HerdrNvimVersion([string]$Root) {
    $text = Get-Content -LiteralPath (Join-Path $Root 'herdr-plugin.toml') -Raw
    if ($text -notmatch '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"') { throw 'No manifest version' }
    return $Matches[1]
}
function Assert-WindowsX64 {
    if ($env:OS -ne 'Windows_NT' -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
        throw 'This installer supports native Windows x64 only. Use an x64 PowerShell process.'
    }
}
function Assert-HerdrNvimBinary([string]$Binary) {
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $Binary
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $p = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $p.StandardOutput.ReadToEndAsync()
        $stderr = $p.StandardError.ReadToEndAsync()
        if (-not $p.WaitForExit(10000)) { $p.Kill(); $p.WaitForExit(); throw 'Binary probe timed out' }
        if ($p.ExitCode -ne 2 -or $stderr.Result -notmatch 'usage: herdr-nvim') { throw 'Binary probe failed (expected herdr-nvim usage, exit 2)' }
    } finally { $p.Dispose() }
}
function Get-HerdrSha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose(); $stream.Dispose() }
}
function Receive-CheckedAsset([string]$Url, [string]$Destination) {
    $hashFile = "$Destination.sha256"
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Destination
    Invoke-WebRequest -UseBasicParsing -Uri "$Url.sha256" -OutFile $hashFile
    $expected = ((Get-Content -LiteralPath $hashFile -Raw).Trim() -split '\s+')[0]
    if ($expected -notmatch '^[0-9a-fA-F]{64}$' -or (Get-HerdrSha256 $Destination) -ne $expected) {
        throw "SHA-256 mismatch: $Url (nothing installed)"
    }
}
function Import-MsvcEnvironment {
    if (Get-Command cl.exe -ErrorAction SilentlyContinue) { return }
    $where = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $where)) { throw 'MSVC not found: run in an x64 Developer PowerShell, or install the C++ build tools manually.' }
    $vs = & $where -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or -not $vs) { throw 'vswhere found no C++ toolchain' }
    $dev = Join-Path ($vs | Select-Object -First 1) 'Common7\Tools\VsDevCmd.bat'
    $lines = & $env:ComSpec /d /s /c "`"`"$dev`" -no_logo -arch=x64 -host_arch=x64 >nul && set`""
    if ($LASTEXITCODE -ne 0) { throw 'VsDevCmd failed' }
    foreach ($line in $lines) {
        if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process') }
    }
}
function Build-HerdrNvim([string]$Root, [string]$Profile = 'release', [string]$Target = 'x86_64-pc-windows-msvc') {
    if (-not (Get-Command cargo.exe -ErrorAction SilentlyContinue)) { throw 'cargo not found. Use a matching release prebuilt, or install Rust MSVC and C++ build tools manually.' }
    Import-MsvcEnvironment
    Push-Location $Root
    try {
        # Cargo's artifact message is authoritative: respects CARGO_TARGET_DIR,
        # target config and custom profile directories. Never guess target/release.
        $lines = @(& cargo.exe build --locked --profile $Profile --target $Target --message-format=json-render-diagnostics)
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
        $exe = $null
        foreach ($line in $lines) {
            try { $message = $line | ConvertFrom-Json } catch { continue }
            if ($message.reason -eq 'compiler-artifact' -and $message.target.name -eq 'herdr-nvim' -and $message.executable) { $exe = $message.executable }
        }
        if (-not $exe -or -not (Test-Path -LiteralPath $exe)) { throw 'Cargo returned no herdr-nvim executable artifact' }
        return $exe
    } finally { Pop-Location }
}
function Set-HerdrNvimBinary([string]$Candidate, [string]$Destination) {
    $dir = Split-Path -Parent $Destination
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $stage = Join-Path $dir ('.new-' + [guid]::NewGuid().ToString('N') + '.exe')
    $old = "$stage.old"
    $committed = $false
    try {
        Copy-Item -LiteralPath $Candidate -Destination $stage
        Assert-HerdrNvimBinary $stage
        if (Test-Path -LiteralPath $Destination) { Move-Item -LiteralPath $Destination -Destination $old }
        try { Move-Item -LiteralPath $stage -Destination $Destination; $committed = $true }
        catch {
            if (Test-Path -LiteralPath $old) {
                try { Move-Item -LiteralPath $old -Destination $Destination }
                catch { throw "Replacement and rollback failed; previous binary is retained at $old. Close active plugin panes before restoring it." }
            }
            throw
        }
    } finally {
        Remove-Item -LiteralPath $stage -Force -ErrorAction SilentlyContinue
        if ($committed) { Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue }
    }
}
