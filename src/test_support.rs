//! Shared test fixtures. Tests redirect process-wide env vars (the daemon
//! runtime dir, the state dir, the nvim config home), so every test that
//! touches any of them goes through the one `TestEnv`, serialized by one lock
//! -- there is no lock order to get wrong.

use std::{
    env,
    ffi::OsString,
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, MutexGuard,
    },
};

use crate::{
    config::{Config, Sidebar},
    daemon::{ask_expr, ensure_daemon, registry, remote_expr, Reply, RPC_TIMEOUT},
    state::{self, TabId},
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static COUNTER: AtomicUsize = AtomicUsize::new(0);

// Keep the *original* Windows Child handles at spawn time, never reopen a
// numeric PID. Even an exited process handle still identifies that process.
// Scoped TestEnv serializes callers and reaps leftover handles after panics.
#[cfg(windows)]
static CHILDREN: Mutex<Vec<(PathBuf, std::process::Child)>> = Mutex::new(Vec::new());

#[cfg(windows)]
pub(crate) fn remember_child(socket: &std::path::Path, child: std::process::Child) {
    CHILDREN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((socket.to_owned(), child));
}

#[cfg(windows)]
fn reap(child: &mut std::process::Child) {
    // Child::kill on Windows uses the stored process HANDLE, not its PID.
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn kill_owned(socket: Option<&std::path::Path>, pid: u32) {
    let mut children = CHILDREN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = children
        .iter()
        .position(|(s, c)| c.id() == pid && socket.is_none_or(|socket| socket == s))
    {
        let (_, mut child) = children.swap_remove(at);
        reap(&mut child);
    }
    // No original handle means no ownership; deliberately do nothing.
}

const VARS: [&str; 7] = [
    "HERDR_NVIM_RUNTIME_DIR",
    "HERDR_NVIM_STATE_DIR",
    "XDG_CONFIG_HOME",
    "HERDR_NVIM_CONFIG",
    "HERDR_NVIM_HOST",
    "HERDR_NVIM_SELF",
    "HERDR_NVIM_PLUGIN_ROOT",
];

/// Points the runtime dir (`<dir>/runtime`, not created up front), the state
/// dir (`<dir>/state`) and an empty nvim config home into a unique temp dir
/// while alive, restoring the prior env and deleting the dir on drop. The
/// config isolation keeps any real user init.lua out of spawned daemons.
pub(crate) struct TestEnv {
    _lock: MutexGuard<'static, ()>,
    old: Vec<Option<OsString>>,
    pub(crate) dir: PathBuf,
}

impl TestEnv {
    pub(crate) fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = env::temp_dir().join(format!(
            "hn-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old = VARS.iter().map(env::var_os).collect();
        env::set_var("HERDR_NVIM_RUNTIME_DIR", dir.join("runtime"));
        env::set_var("HERDR_NVIM_STATE_DIR", dir.join("state"));
        env::set_var("XDG_CONFIG_HOME", dir.join("xdg-config"));
        for var in &VARS[3..] {
            env::remove_var(var);
        }
        Self {
            _lock: lock,
            old,
            dir,
        }
    }

    pub(crate) fn runtime_dir(&self) -> PathBuf {
        self.dir.join("runtime")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        #[cfg(windows)]
        for (_, mut child) in CHILDREN.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            reap(&mut child);
        }
        for (var, old) in VARS.iter().zip(&self.old) {
            match old {
                Some(value) => env::set_var(var, value),
                None => env::remove_var(var),
            }
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub(crate) fn nvim_available() -> bool {
    #[cfg(not(windows))]
    let which = "which";
    #[cfg(windows)]
    let which = "where.exe";
    Command::new(which)
        .arg("nvim")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Whether the OS still runs `pid`. The test process is the daemons'
/// parent and never reaps them, so on Unix an exited daemon lingers as a
/// zombie -- that counts as dead.
pub(crate) fn process_alive(pid: u32) -> bool {
    #[cfg(not(windows))]
    {
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("run ps");
        let stat = String::from_utf8_lossy(&output.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .expect("run tasklist");
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    }
}

pub(crate) fn kill(pid: u32) {
    #[cfg(not(windows))]
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    #[cfg(windows)]
    kill_owned(None, pid);
}

#[cfg(windows)]
#[test]
fn teardown_without_an_owned_handle_does_not_kill_a_live_pid() {
    let _env = TestEnv::new();
    struct Owned(std::process::Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut unrelated = Owned(
        Command::new("nvim.exe")
            .args(["--headless", "--clean", "-i", "NONE"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("required nvim.exe"),
    );
    assert!(unrelated.0.try_wait().unwrap().is_none());
    // A stale/mismatched TestDaemon identity must not grant ownership of a PID.
    let stale = TestDaemon {
        tab: TabId::new("not-owned"),
        socket: _env.dir.join("not-owned"),
        pid: unrelated.0.id(),
    };
    drop(stale);
    assert!(
        unrelated.0.try_wait().unwrap().is_none(),
        "teardown killed an unrelated live process without an owned handle"
    );
    let socket = _env.dir.join("exited");
    let mut exited = Command::new("nvim.exe")
        .args(["--headless", "--clean", "-i", "NONE", "+qa!"])
        .spawn()
        .unwrap();
    let dead_pid = exited.id();
    exited.wait().unwrap();
    remember_child(&socket, exited);
    drop(TestDaemon {
        tab: TabId::new("exited"),
        socket,
        pid: dead_pid,
    });
    assert!(CHILDREN.lock().unwrap().is_empty());
    assert!(
        unrelated.0.try_wait().unwrap().is_none(),
        "an exited handle must not affect another process"
    );
    let socket = _env.dir.join("owned");
    let owned = Command::new("nvim.exe")
        .args(["--headless", "--clean", "-i", "NONE"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = owned.id();
    remember_child(&socket, owned);
    drop(TestDaemon {
        tab: TabId::new("changed"),
        socket: _env.dir.join("wrong-socket"),
        pid,
    });
    assert!(
        CHILDREN.lock().unwrap()[0].1.try_wait().unwrap().is_none(),
        "changed socket identity must not terminate the owned child"
    );
    drop(TestDaemon {
        tab: TabId::new("owned"),
        socket,
        pid,
    });
    assert!(CHILDREN.lock().unwrap().is_empty());
    assert!(unrelated.0.try_wait().unwrap().is_none());
}

/// nvim stops its jobs on exit, but asynchronously; give it a moment.
pub(crate) fn wait_dead(pid: u32) {
    for _ in 0..40 {
        if !process_alive(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// A real daemon spawned for a test tab, with the pid it reported. Windows
/// teardown requires its original Child handle in CHILDREN, never PID lookup;
/// TestEnv also reaps retained handles if startup/assertions fail.
pub(crate) struct TestDaemon {
    pub(crate) tab: TabId,
    pub(crate) socket: PathBuf,
    pub(crate) pid: u32,
}

impl TestDaemon {
    pub(crate) fn spawn(tab: &str, config: &Config) -> Self {
        let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let socket = ensure_daemon(tab, &plugin_root, config, &plugin_root).expect("ensure_daemon");
        let pid = ask_expr(&socket, "getpid()", &config.sidebar)
            .expect("daemon pid")
            .parse()
            .expect("numeric pid");
        // Give the daemon a sidebar state file so its cleanup is observable.
        let state_file = state::state_path(tab);
        fs::create_dir_all(state_file.parent().unwrap()).unwrap();
        fs::write(&state_file, b"{}").unwrap();
        Self {
            tab: TabId::new(tab),
            socket,
            pid,
        }
    }

    /// Evaluate `expr` on the daemon; panics if it does not answer.
    pub(crate) fn eval(&self, expr: &str) -> String {
        ask_expr(&self.socket, expr, &Sidebar::default())
            .unwrap_or_else(|error| panic!("{} daemon did not answer {expr}: {error}", self.tab))
    }

    /// Leave a modified, listed buffer in the daemon. `noswapfile`: parallel
    /// tests may point HOME somewhere unwritable, and a failed swap-file
    /// creation would abort the edit.
    pub(crate) fn dirty_a_buffer(&self) {
        self.eval(r#"execute('setlocal noswapfile | call setline(1, "unsaved")')"#);
    }

    /// SIGSTOP the daemon: it keeps its socket, which still accepts
    /// connections, but answers nothing -- a wedged daemon (issue #61).
    #[cfg(unix)]
    pub(crate) fn hang(&self) {
        self.signal("-STOP");
    }

    /// Busy-loop the daemon's main loop, like a plugin stuck in a loop: its
    /// socket still takes connections, nothing answers, and it does not act
    /// on SIGTERM either (nvim handles signals on that same loop).
    #[cfg(unix)]
    pub(crate) fn busy(&self) {
        let reply = crate::daemon::remote_send(
            &self.socket,
            "<cmd>lua while true do end<cr>",
            &Sidebar::default(),
            RPC_TIMEOUT,
        );
        assert!(matches!(reply, Ok(Reply::Answered(_))), "{reply:?}");
    }

    /// SIGCONT a daemon `hang` stopped.
    #[cfg(unix)]
    pub(crate) fn resume(&self) {
        self.signal("-CONT");
    }

    #[cfg(unix)]
    fn signal(&self, signal: &str) {
        Command::new("kill")
            .args([signal, &self.pid.to_string()])
            .status()
            .expect("run kill");
    }

    pub(crate) fn assert_alive(&self) {
        let tab = &self.tab;
        assert!(process_alive(self.pid), "{tab} daemon process died");
        assert_eq!(self.eval("1+1"), "2", "{tab} daemon stopped answering");
        assert!(
            registry::registry_path(&tab.key()).exists(),
            "{tab} registry entry removed"
        );
        assert!(
            state::state_path(tab.as_str()).exists(),
            "{tab} state file removed"
        );
    }

    pub(crate) fn assert_gone(&self) {
        let tab = &self.tab;
        assert!(
            !process_alive(self.pid),
            "{tab} daemon process still running"
        );
        assert!(
            matches!(
                remote_expr(&self.socket, "1+1", &Sidebar::default(), RPC_TIMEOUT),
                Ok(Reply::Refused)
            ),
            "{tab} daemon still answering"
        );
        assert!(
            !registry::registry_path(&tab.key()).exists(),
            "{tab} registry entry left behind"
        );
        assert!(
            !state::state_path(tab.as_str()).exists(),
            "{tab} state file left behind"
        );
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        #[cfg(windows)]
        kill_owned(Some(&self.socket), self.pid);
        #[cfg(not(windows))]
        if process_alive(self.pid) {
            kill(self.pid);
        }
        #[cfg(not(windows))]
        let _ = state::remove_file_if_exists(&self.socket);
    }
}
