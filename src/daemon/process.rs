//! A daemon seen as an OS process rather than over its socket: its memory
//! (with its LSPs and jobs) and age for `herdr-nvim daemons`, and finding and
//! killing one that does not answer over its socket or will not quit.
//!
//! A process inspection that fails is an error, never taken for a daemon
//! that is gone: only an inspection that succeeds and finds no such
//! process says it is.

use std::{collections::HashMap, path::Path, process::Command};
#[cfg(unix)]
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};

struct Proc {
    ppid: u32,
    rss_bytes: u64,
    uptime_secs: u64,
    /// The command line (CIM on Windows, ps on Unix).
    args: String,
}

/// One snapshot of the OS process table: enough to sum a daemon's memory
/// together with its descendants' (LSP servers, jobs) and read its age.
pub(crate) struct ProcessTable {
    procs: HashMap<u32, Proc>,
    children: HashMap<u32, Vec<u32>>,
}

impl ProcessTable {
    pub(crate) fn load() -> Result<Self> {
        let procs = read_processes().context("could not read the process table")?;
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (&pid, proc_) in &procs {
            if proc_.ppid != pid {
                children.entry(proc_.ppid).or_default().push(pid);
            }
        }
        Ok(Self { procs, children })
    }

    /// `pid` and all of its descendants that are still running.
    pub(crate) fn tree(&self, pid: u32) -> Vec<u32> {
        let mut seen = vec![pid];
        let mut index = 0;
        while index < seen.len() {
            for &child in self.children.get(&seen[index]).into_iter().flatten() {
                // Guards against ppid cycles from pid reuse (Windows).
                if !seen.contains(&child) {
                    seen.push(child);
                }
            }
            index += 1;
        }
        seen
    }

    pub(crate) fn tree_rss(&self, pid: u32) -> Option<u64> {
        self.procs.get(&pid)?;
        Some(
            self.tree(pid)
                .iter()
                .filter_map(|pid| self.procs.get(pid))
                .map(|proc_| proc_.rss_bytes)
                .sum(),
        )
    }

    #[cfg(test)]
    pub(crate) fn rss(&self, pid: u32) -> Option<u64> {
        self.procs.get(&pid).map(|proc_| proc_.rss_bytes)
    }

    pub(crate) fn uptime(&self, pid: u32) -> Option<u64> {
        self.procs.get(&pid).map(|proc_| proc_.uptime_secs)
    }

    /// Every nvim process listening on `socket` (see `is_daemon`), sorted.
    /// Normally one: the daemon. Two when a launcher shim starts the real
    /// nvim as its child instead of `exec`ing it. On Windows this is a
    /// conservative liveness check only, never permission to kill a PID.
    pub(crate) fn listeners(&self, socket: &Path) -> Result<Vec<u32>> {
        let mut pids = Vec::new();
        // The snapshot's command lines rule out all but a few.
        for (&pid, proc_) in &self.procs {
            #[cfg(unix)]
            let matches = listens_on(&proc_.args, socket) && is_daemon(pid, socket)?;
            // Used only to establish possible liveness, never to authorize a kill.
            #[cfg(windows)]
            let matches = listens_on(&proc_.args, socket);
            if matches {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        Ok(pids)
    }

    /// The daemon listening on `socket`: its one listener or, when a shim
    /// and the nvim it started both listen, the innermost one. `None` when
    /// nothing listens.
    pub(crate) fn daemon(&self, socket: &Path) -> Result<Option<u32>> {
        let pids = self.listeners(socket)?;
        let innermost: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|&pid| {
                !pids
                    .iter()
                    .any(|&other| other != pid && self.tree(pid).contains(&other))
            })
            .collect();
        match (&pids[..], &innermost[..]) {
            (_, [pid]) => Ok(Some(*pid)),
            ([], _) => Ok(None),
            _ => bail!(
                "found {} nvim processes listening on {} (pids {pids:?})",
                pids.len(),
                socket.display()
            ),
        }
    }
}

/// Whether the command line `args` has `--listen <socket>` as a whole
/// argument pair after its program, so an `nvim --server <socket>` client,
/// a longer socket path sharing the prefix, or a zombie (whose command line
/// `ps` shows as `[nvim]` or `(nvim)`) never match. Which program it is, is
/// for `is_nvim` to say.
fn listens_on(args: &str, socket: &Path) -> bool {
    let pair = format!(" --listen {}", socket.to_string_lossy());
    let plain = args
        .match_indices(&pair)
        .any(|(at, _)| matches!(args[at + pair.len()..].chars().next(), None | Some(' ')));
    #[cfg(windows)]
    return plain || args.contains(&format!(" --listen \"{}\"", socket.to_string_lossy()));
    #[cfg(not(windows))]
    plain
}

/// Whether `ps`'s command name (`comm`: the executable, unlike `args`,
/// whatever argv[0] says, and on macOS a full path that may hold spaces)
/// is an nvim.
#[cfg_attr(windows, allow(dead_code))]
fn is_nvim(command_name: &str) -> bool {
    Path::new(command_name)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("nvim"))
}

/// One running process's `ps` field, `None` once it has exited.
#[cfg(unix)]
fn ps_field(pid: u32, field: &str) -> Result<Option<String>> {
    let output = Command::new("ps")
        .args(["-ww", "-o", field, "-p", &pid.to_string()])
        .output()
        .context("could not run ps")?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() {
        return Ok(Some(text));
    }
    // For a pid that no process has, `ps` fails without a word.
    if text.is_empty() && output.stderr.is_empty() {
        return Ok(None);
    }
    bail!(
        "ps -o {field} -p {pid} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

/// Whether `pid` is (still) the nvim listening on `socket`: the one test of
/// a daemon's identity, by its current command line and executable.
#[cfg(unix)]
fn is_daemon(pid: u32, socket: &Path) -> Result<bool> {
    if !ps_field(pid, "args=")?.is_some_and(|args| listens_on(&args, socket)) {
        return Ok(false);
    }
    Ok(ps_field(pid, "comm=")?.is_some_and(|name| is_nvim(&name)))
}

/// Wait up to `timeout` for `pid`, the nvim listening on `socket`, to exit
/// (or to stop being that process); `true` once it has.
#[cfg(unix)]
pub(crate) fn wait_gone(pid: u32, socket: &Path, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        if !is_daemon(pid, socket)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(EXIT_POLL_INTERVAL);
    }
}

/// How long a daemon gets to exit after SIGTERM, and then after SIGKILL.
#[cfg(unix)]
const EXIT_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(unix)]
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Kill the nvim daemon listening on `socket` -- one that does not answer
/// over it, or will not quit -- and return its pid once it is gone. SIGTERM
/// first (with SIGCONT, so a stopped daemon gets to act on it), SIGKILL if
/// it is still there after `EXIT_TIMEOUT`: a busy-looping nvim needs that,
/// as it handles SIGTERM on the very loop that is stuck. A daemon that
/// leads its own process group, as a spawned one does (`detach_command`),
/// is signalled with its whole group, so its LSP servers (`vim.system`
/// children) go too; `jobstart()` jobs, which nvim puts in groups of their
/// own, do not. The pid comes from the process table
/// (`ProcessTable::daemon`), and right before each round of signals it is
/// checked again to still be that nvim, so a pid since reused by another
/// process is never signalled. `None` when the daemon had already exited,
/// before any signal. Several unrelated listeners are an error that signals
/// nothing.
#[cfg(unix)]
pub(crate) fn kill_daemon(socket: &Path) -> Result<Option<u32>> {
    let Some(pid) = ProcessTable::load()?.daemon(socket)? else {
        return Ok(None);
    };
    let Some(pgid) = ps_field(pid, "pgid=")? else {
        return Ok(None);
    };
    let target = if pgid.parse() == Ok(pid) {
        format!("-{pid}")
    } else {
        pid.to_string()
    };
    for (round, signals) in [&["TERM", "CONT"][..], &["KILL"]].into_iter().enumerate() {
        if !is_daemon(pid, socket)? {
            return Ok((round > 0).then_some(pid));
        }
        for signal in signals {
            // Whether it took is for the wait below to say.
            Command::new("kill")
                .args(["-s", signal, "--", &target])
                .output()
                .context("could not run kill")?;
        }
        if wait_gone(pid, socket, EXIT_TIMEOUT)? {
            return Ok(Some(pid));
        }
    }
    bail!(
        "nvim daemon pid {pid} on {} survived SIGKILL",
        socket.display()
    )
}

#[cfg(windows)]
pub(crate) fn kill_daemon(socket: &Path) -> Result<Option<u32>> {
    bail!(
        "cannot force-stop an nvim daemon on Windows; \
         end the nvim.exe listening on {} by hand",
        socket.display()
    )
}

/// Unix: one `ps` call; `rss` is in KiB, `etime` is `[[dd-]hh:]mm:ss`, and
/// `args` comes last and whole (`-ww`: never cut to the terminal width).
#[cfg(not(windows))]
fn read_processes() -> Result<HashMap<u32, Proc>> {
    let output = Command::new("ps")
        .args(["-A", "-ww", "-o", "pid=,ppid=,rss=,etime=,args="])
        .output()
        .context("could not run ps")?;
    if !output.status.success() {
        bail!(
            "ps -A failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut rest = line;
            let mut field = || {
                let trimmed = rest.trim_start();
                let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
                rest = &trimmed[end..];
                Some(&trimmed[..end]).filter(|field| !field.is_empty())
            };
            let pid = field()?.parse().ok()?;
            let ppid = field()?.parse().ok()?;
            let rss_kib: u64 = field()?.parse().ok()?;
            let uptime_secs = parse_etime(field()?)?;
            Some((
                pid,
                Proc {
                    ppid,
                    rss_bytes: rss_kib * 1024,
                    uptime_secs,
                    args: rest.trim().to_owned(),
                },
            ))
        })
        .collect())
}

/// Windows: a bounded PowerShell CIM snapshot for inventory and conservative
/// liveness checks (about 1s locally); working set stands in for RSS.
#[cfg(windows)]
fn read_processes() -> Result<HashMap<u32, Proc>> {
    // ErrorAction Stop matters: PowerShell otherwise exits 0 after a CIM error.
    // A remote-expr client may exit while CIM enumerates it. Requery that PID:
    // only confirmed absence is Gone; unreadable live processes still fail closed.
    let script = r#"$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$now=Get-Date
$rows=@(Get-CimInstance Win32_Process -ErrorAction Stop | ForEach-Object {
    $proc=$_
    if ($proc.Name -like 'nvim*' -and -not $proc.CommandLine) {
        $proc=Get-CimInstance Win32_Process -Filter "ProcessId = $([uint32]$proc.ProcessId)" -ErrorAction Stop
        if ($null -eq $proc) { return }
        if (-not $proc.CommandLine) { throw 'Cannot inspect an nvim command line; refusing to assume it exited' }
    }
    @{pid=[uint32]$proc.ProcessId; ppid=[uint32]$proc.ParentProcessId; rss=[uint64]$proc.WorkingSetSize; up=if ($proc.CreationDate) {[uint64][Math]::Max(0,($now-$proc.CreationDate).TotalSeconds)} else {0}; args=[string]$proc.CommandLine}
})
ConvertTo-Json -Compress -InputObject $rows"#;
    use std::{
        process::Stdio,
        thread::sleep,
        time::{Duration, Instant},
    };
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run powershell.exe to inspect daemon processes")?;
    let stdout = super::drain(child.stdout.take().expect("piped stdout"));
    let stderr = super::drain(child.stderr.take().expect("piped stderr"));
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => sleep(Duration::from_millis(20)),
            _ => {
                super::kill_client(&mut child)?;
                bail!("process inspection timed out or failed; daemon state is unknown, nothing replaced");
            }
        }
    };
    let drained = Instant::now() + Duration::from_secs(1);
    let text = super::output(stdout, drained);
    let error = super::output(stderr, drained);
    if !status.success() {
        bail!("Get-CimInstance failed: {error}");
    }
    #[derive(serde::Deserialize)]
    struct Row {
        pid: u32,
        ppid: u32,
        rss: u64,
        up: u64,
        args: String,
    }
    let rows: Vec<Row> = serde_json::from_str(&text)
        .context("invalid process snapshot (nothing considered gone)")?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.pid,
                Proc {
                    ppid: r.ppid,
                    rss_bytes: r.rss,
                    uptime_secs: r.up,
                    args: r.args,
                },
            )
        })
        .collect())
}

/// `ps` elapsed time `[[dd-]hh:]mm:ss` in seconds.
#[cfg_attr(windows, allow(dead_code))]
fn parse_etime(etime: &str) -> Option<u64> {
    let (days, clock) = match etime.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, etime),
    };
    let mut secs = 0;
    for part in clock.split(':') {
        secs = secs * 60 + part.parse::<u64>().ok()?;
    }
    Some(days * 86_400 + secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ps_elapsed_time() {
        assert_eq!(parse_etime("05:07"), Some(307));
        assert_eq!(parse_etime("03:31:00"), Some(12_660));
        assert_eq!(parse_etime("2-00:00:01"), Some(172_801));
        assert_eq!(parse_etime("garbage"), None);
    }

    #[test]
    fn a_daemon_is_an_nvim_with_the_listen_argv_pair() {
        let socket = Path::new("/tmp/herdr-nvim/w1_t1.sock");
        for args in [
            "nvim --headless --listen /tmp/herdr-nvim/w1_t1.sock --cmd set rtp+=/x",
            "/Applications/Neovim Nightly/bin/nvim --headless --listen /tmp/herdr-nvim/w1_t1.sock",
        ] {
            assert!(listens_on(args, socket), "{args}");
        }
        for args in [
            "nvim --headless --server /tmp/herdr-nvim/w1_t1.sock --remote-expr 1+1",
            "nvim --headless --listen /tmp/herdr-nvim/w1_t10.sock",
            "nvim --headless --listen /tmp/herdr-nvim/w1_t1.sock.bak",
            "[nvim] <defunct>",
            "",
        ] {
            assert!(!listens_on(args, socket), "{args}");
        }
        for name in [
            "nvim",
            "/opt/homebrew/bin/nvim",
            "/Applications/My Tools/nvim-0.12",
        ] {
            assert!(is_nvim(name), "{name}");
        }
        for name in ["vim", "sh", "/usr/bin/lvim", ""] {
            assert!(!is_nvim(name), "{name}");
        }
    }
}
