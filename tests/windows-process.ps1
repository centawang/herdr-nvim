# Read-only validation of the exact CIM script embedded in the Rust backend.
# Does not contact/stop a daemon. Output never includes users' command lines.
param([switch]$Snapshot)
$ErrorActionPreference = 'Stop'
$source = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../src/daemon/process.rs') -Raw
if ($source -notmatch '(?s)let script = r#"(.*?)"#;') { throw 'CIM script not found' }
$script = $Matches[1]
$tokens = $null; $errors = $null
$null = [Management.Automation.Language.Parser]::ParseInput($script, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw $errors }
# Drive the exact embedded script with a fake CIM provider before reading the OS.
# A short-lived --remote-expr client can exit between enumeration and property
# retrieval. Only a successful PID requery proving absence permits omission.
function Test-Snapshot([string]$Case, [bool]$MustFail) {
    $script:queries = 0
    function Get-CimInstance {
        param($ClassName, $ErrorAction, $Filter)
        $script:queries++
        if ($Case -eq 'enumeration-error') { throw 'fixture: CIM unavailable' }
        if (-not $Filter) {
            return @(
                [pscustomobject]@{Name='system.exe'; ProcessId=1; ParentProcessId=0; WorkingSetSize=1; CreationDate=$null; CommandLine='system'},
                [pscustomobject]@{Name='nvim.exe'; ProcessId=42; ParentProcessId=1; WorkingSetSize=1; CreationDate=$null; CommandLine=$null}
            )
        }
        if ($Filter -ne 'ProcessId = 42') { throw "wrong PID filter: $Filter" }
        if ($Case -eq 'requery-error') { throw 'fixture: CIM requery unavailable' }
        if ($Case -eq 'exited') { return }
        $line = if ($Case -eq 'readable') { 'nvim.exe --headless --listen fixture-pipe' } else { $null }
        return [pscustomobject]@{Name='nvim.exe'; ProcessId=42; ParentProcessId=1; WorkingSetSize=1; CreationDate=$null; CommandLine=$line}
    }
    $failed = $false; $text = $null
    try { $text = & ([scriptblock]::Create($script)) } catch { $failed = $true }
    if ($failed -ne $MustFail) { throw "CIM fixture $Case : failure=$failed expected=$MustFail" }
    if (-not $MustFail) {
        $rows = ConvertFrom-Json -InputObject ($text -join "`n")
        $wanted = if ($Case -eq 'exited') { 1 } else { 2 }
        if ($rows.Count -ne $wanted -or $script:queries -ne 2) { throw "CIM fixture $Case : rows=$($rows.Count), queries=$script:queries; expected $wanted rows, 2 queries" }
        if ($Case -eq 'readable' -and $rows[1].args -ne 'nvim.exe --headless --listen fixture-pipe') { throw 'refreshed command line lost' }
    }
}
Test-Snapshot 'exited' $false
Test-Snapshot 'readable' $false
Test-Snapshot 'unreadable' $true
Test-Snapshot 'enumeration-error' $true
Test-Snapshot 'requery-error' $true
Write-Host 'CIM snapshot fixtures: 5 passed; 0 failed (unreadable/query errors remain unknown)'
if ($Snapshot) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $text = & powershell.exe -NoProfile -NonInteractive -Command $script
    if ($LASTEXITCODE -ne 0) { throw 'CIM snapshot failed (must not be treated as Gone)' }
    $rows = ConvertFrom-Json -InputObject ($text -join "`n")
    if (-not $rows.Count) { throw 'No process rows' }
    foreach ($row in $rows) {
        if ($null -eq $row.pid -or $null -eq $row.args -or $null -eq $row.up) { throw 'Missing process fields' }
    }
    Write-Host "CIM JSON snapshot: ok; $($rows.Count) rows; $($clock.ElapsedMilliseconds) ms (no termination)"
} else { Write-Host 'CIM script parse: ok' }
