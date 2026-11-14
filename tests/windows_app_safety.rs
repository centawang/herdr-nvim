#![cfg(windows)]
//! Public entry/installer regression with private real Neovim Child handles.
use serde_json::{json, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread::sleep,
    time::Duration,
};
#[path = "../src/daemon/namespace.rs"]
mod namespace;

fn ok(out: Output) -> String {
    assert!(
        out.status.success(),
        "exit={} stdout={} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn run(exe: &Path, args: &[&str]) -> Output {
    Command::new(exe).args(args).output().unwrap()
}
struct Nvim {
    child: Child,
    pipe: PathBuf,
}
impl Nvim {
    fn eval(&self, expr: &str) -> String {
        ok(Command::new("nvim.exe")
            .args(["--headless", "--clean", "--server"])
            .arg(&self.pipe)
            .args(["--remote-expr", expr])
            .output()
            .unwrap())
        .trim()
        .to_owned()
    }
}
impl Drop for Nvim {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn owned(home: &Path, entry: &Path) {
    let hash = ok(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command"]).arg(format!("[BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([IO.File]::ReadAllBytes('{}'))).Replace('-','').ToLowerInvariant()",entry.with_extension("app.json").display())).output().unwrap());
    for name in ["herdr-nvim", "herdr-nvim-header"] {
        let dir = home.join("mods").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(".herdr-nvim"),
            json!({"owner":"herdr-nvim.windows.v1","entry":entry,"sidecar_sha256":hash.trim()})
                .to_string(),
        )
        .unwrap();
    }
}
fn entry(root: &Path, home: &Path, name: &str, core: &Value, fake: &Path) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("herdr-nvim.exe");
    fs::copy(env!("CARGO_BIN_EXE_herdr-nvim"), &exe).unwrap();
    fs::write(dir.join("runtime-stamp"), name).unwrap();
    fs::write(
        exe.with_extension("app.json"),
        json!({"plugin_root":dir,"app_home":home,"app_bin":fake,"core":core}).to_string(),
    )
    .unwrap();
    exe
}
// Explicit new installation, never used to repair an existing entry/sidecar.
fn for_home(core: &Value, home: &Path) -> Value {
    let captured: Value = serde_json::from_str(&ok(Command::new(env!("CARGO_BIN_EXE_herdr-nvim"))
        .arg("app-core-binding")
        .env("HERDR_APP_HOME", home)
        .output()
        .unwrap()))
    .unwrap();
    let mut result = core.clone();
    result["app_home_identity"] = captured["app_home_identity"].clone();
    result
}
fn watch(entry: &Path, root: &Path, tab: &str) -> PathBuf {
    let ctx = json!({"space":"sp","tab":tab,"cwd":root}).to_string();
    let value: Value =
        serde_json::from_str(&ok(run(entry, &["app-current", "--ctx", &ctx]))).unwrap();
    PathBuf::from(value["watch"].as_str().expect("watch path"))
}
fn daemon(root: &Path, entry: &Path, tab: &str) -> (Nvim, PathBuf, PathBuf) {
    let watch = watch(entry, root, tab);
    let scope = watch
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .file_name()
        .unwrap();
    let base = root.join("XDG_RUNTIME_DIR/herdr-nvim/app");
    let dir = if scope.to_string_lossy().starts_with("instance-") {
        base.join(scope)
    } else {
        base
    };
    fs::create_dir_all(&dir).unwrap();
    let key = format!("sp_{tab}");
    let pipe = namespace::pipe(&dir, &key);
    let child = Command::new("nvim.exe")
        .args(["--headless", "--clean", "-i", "NONE", "--listen"])
        .arg(&pipe)
        .args([
            "--cmd",
            &format!("lua vim.opt.rtp:append({:?})", env!("CARGO_MANIFEST_DIR")),
        ])
        .env("HERDR_TAB_ID", format!("sp:{tab}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let d = Nvim { child, pipe };
    sleep(Duration::from_millis(400));
    assert_eq!(d.eval("1+1"), "2");
    let marker = dir.join(format!("{key}.pipe"));
    fs::write(
        &marker,
        json!({"version":2,"tab":format!("sp:{tab}"),"endpoint":d.pipe}).to_string(),
    )
    .unwrap();
    fs::write(&watch, b"{\"file\":\"A-unsaved\"}").unwrap();
    d.eval("execute('call setline(1, \"private unsaved\")')");
    d.eval("luaeval(\"require('herdr-nvim.comments').add(vim.api.nvim_get_current_buf(), {start_line=1,end_line=1}, 'pending private review')\")");
    (d, marker, watch)
}
#[test]
fn app_instances_preserve_foreign_work_and_reject_restored_home() {
    let root = env::temp_dir().join(format!("hn-app-safety-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    // One user and production default base; no per-instance runtime override.
    for name in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_RUNTIME_DIR",
        "TEMP",
        "TMP",
    ] {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        env::set_var(name, path);
    }
    for name in [
        "HERDR_NVIM_RUNTIME_DIR",
        "HERDR_NVIM_STATE_DIR",
        "HERDR_NVIM_HOST",
        "HERDR_NVIM_SELF",
        "HERDR_NVIM_INSTANCE",
        "HERDR_NVIM_CORE_BINDING",
        "HC_SOCKET",
    ] {
        env::remove_var(name);
    }
    env::set_var("HERDR_NVIM_CONFIG", root.join("no-config"));
    let fake = root.join("fake-core.exe");
    ok(Command::new("rustc.exe")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake-core.rs"))
        .args(["-o"])
        .arg(&fake)
        .output()
        .unwrap());
    env::set_var("HERDR_NVIM_HC", &fake);
    let a = root.join("app A");
    let b = root.join("app B");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("keys.json"), "{\"keep\":\"A\"}").unwrap();
    fs::write(b.join("keys.json"), "{\"keep\":\"B\"}").unwrap();
    env::set_var("HC_SESSION", "A");
    env::set_var("HERDR_APP_HOME", &a);
    env::set_var("HC_SOCKET", root.join("Core.socket"));
    let core_a: Value = serde_json::from_str(&ok(run(
        Path::new(env!("CARGO_BIN_EXE_herdr-nvim")),
        &["app-core-binding"],
    )))
    .unwrap();
    env::set_var("HC_SESSION", "B");
    env::set_var("HERDR_APP_HOME", &b);
    let core_b: Value = serde_json::from_str(&ok(run(
        Path::new(env!("CARGO_BIN_EXE_herdr-nvim")),
        &["app-core-binding"],
    )))
    .unwrap();
    let ae = entry(&root, &a, "runtime-A-v1", &core_a, &fake);
    let be = entry(&root, &b, "runtime-B", &core_b, &fake);
    let (mut nvim, marker, state) = daemon(&root, &ae, "same");
    let (mut other, marker2, state2) = daemon(&root, &ae, "onlyA");
    let before = fs::read(&marker).unwrap();
    // Home alone is not core identity: the same home in another core must not
    // discover A's dirty daemons either.
    let other_core = entry(
        &root,
        &a,
        "same-home-other-core",
        &for_home(&core_b, &a),
        &fake,
    );
    ok(run(&other_core, &["app-watch"]));
    assert!(
        nvim.child.try_wait().unwrap().is_none(),
        "same home/different core reaped A"
    );
    let same_core_other_home = entry(
        &root,
        &b,
        "same-core-other-home",
        &for_home(&core_a, &b),
        &fake,
    );
    assert_ne!(watch(&same_core_other_home, &root, "same"), state);
    let contents = fs::read(&state).unwrap();
    // B's same tab must have its own current-file state, not A's file.
    let bstate = watch(&be, &root, "same");
    let (mut bnvim, bmarker, _) = daemon(&root, &be, "same");
    // Empty B core has no A tabs, but must never reconcile A as orphan.
    ok(run(&be, &["app-watch"]));
    assert!(
        nvim.child.try_wait().unwrap().is_none(),
        "B reconcile killed A's dirty daemon"
    );
    assert_ne!(bstate, state);
    assert!(
        bnvim.child.try_wait().unwrap().is_some(),
        "B's own orphan was not reconciled"
    );
    assert!(!bmarker.exists());
    assert_eq!(
        fs::read_to_string(b.join("last-core-session")).unwrap(),
        "B"
    );
    owned(&b, &be);
    ok(Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-app/install.ps1"))
        .arg("-AppHome")
        .arg(&b)
        .arg("-AppBin")
        .arg(&fake)
        .arg("-Uninstall")
        .output()
        .unwrap());
    assert!(nvim.child.try_wait().unwrap().is_none());
    assert!(other.child.try_wait().unwrap().is_none());
    assert_eq!(nvim.eval("getline(1)"), "private unsaved");
    assert_eq!(
        nvim.eval("luaeval(\"package.loaded['herdr-nvim.comments'].list()[1].text\")"),
        "pending private review"
    );
    assert_eq!(fs::read(&marker).unwrap(), before);
    assert_eq!(fs::read(&state).unwrap(), contents);
    assert!(marker2.exists() && state2.exists());
    // Same home/session, a different immutable runtime, still reaches same daemon.
    let upgraded = entry(&root, &a, "runtime-A-v2", &core_a, &fake);
    assert_eq!(watch(&upgraded, &root, "same"), state);
    let ctx = json!({"space":"sp","tab":"same","cwd":root}).to_string();
    ok(run(
        &upgraded,
        &["fs", "{\"op\":\"defaults\"}", "--ctx", &ctx],
    ));
    assert_eq!(
        nvim.eval("getpid()"),
        nvim.child.id().to_string(),
        "upgrade must reuse live daemon"
    );
    let refused = run(&upgraded, &["daemons", "stop", "--all"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("unsaved"));
    assert_eq!(
        fs::read_to_string(a.join("last-core-session")).unwrap(),
        "A",
        "caller HC_SESSION=B must not redirect installed A"
    );
    // Installation cannot turn a selector change into a new namespace while
    // the old instance still owns dirty work.
    owned(&a, &ae);
    let ownership = fs::read(a.join("mods/herdr-nvim/.herdr-nvim")).unwrap();
    let header = fs::read(a.join("mods/herdr-nvim-header/.herdr-nvim")).unwrap();
    for (selector, changed_value) in [
        ("HC_SESSION", "B".to_owned()),
        ("HC_SESSION", "a".to_owned()),
        (
            "HC_SOCKET",
            core_a["selectors"]["HC_SOCKET"]
                .as_str()
                .unwrap()
                .to_lowercase(),
        ),
        (
            "XDG_RUNTIME_DIR",
            core_a["selectors"]["XDG_RUNTIME_DIR"]
                .as_str()
                .unwrap()
                .to_lowercase(),
        ),
    ] {
        let mut cmd = Command::new("powershell.exe");
        cmd.env("HC_SESSION", "A").env(selector, &changed_value);
        let changed = cmd
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-app/install.ps1"))
            .arg("-AppHome")
            .arg(&a)
            .arg("-AppBin")
            .arg(&fake)
            .arg("-Binary")
            .arg(env!("CARGO_BIN_EXE_herdr-nvim"))
            .arg("-RuntimeBase")
            .arg(root.join("unused-runtime"))
            .output()
            .unwrap();
        println!(
            "installer selector {selector}={changed_value}: {}",
            changed.status
        );
        assert!(
            !changed.status.success(),
            "selector drift accepted: {selector}; {}",
            String::from_utf8_lossy(&changed.stdout)
        );
        assert!(
            String::from_utf8_lossy(&changed.stderr).contains("Core binding/config changed"),
            "{}",
            String::from_utf8_lossy(&changed.stderr)
        );
        assert!(
            !root.join("unused-runtime").exists(),
            "must reject before runtime staging"
        );
        assert_eq!(
            fs::read_dir(a.join("mods")).unwrap().count(),
            2,
            "must reject before mod staging/swap"
        );
        assert_eq!(
            fs::read(a.join("mods/herdr-nvim/.herdr-nvim")).unwrap(),
            ownership
        );
        assert_eq!(
            fs::read(a.join("mods/herdr-nvim-header/.herdr-nvim")).unwrap(),
            header
        );
        assert_eq!(fs::read(&marker).unwrap(), before);
        assert_eq!(fs::read(&state).unwrap(), contents);
        assert!(nvim.child.try_wait().unwrap().is_none());
        assert_eq!(nvim.eval("getpid()"), nvim.child.id().to_string());
        assert_eq!(nvim.eval("getline(1)"), "private unsaved");
        assert_eq!(
            nvim.eval("luaeval(\"package.loaded['herdr-nvim.comments'].list()[1].text\")"),
            "pending private review"
        );
    }
    // Ordinary case aliases are the same actual directory, not a new instance.
    let alias = PathBuf::from(a.to_string_lossy().to_lowercase());
    let alias_entry = entry(&root, &alias, "home-alias", &core_a, &fake);
    assert_eq!(watch(&alias_entry, &root, "same"), state);
    ok(run(&ae, &["app-home-check", alias.to_str().unwrap()]));
    let alias_uninstall = uninstall(&alias, &fake);
    assert!(!alias_uninstall.status.success());
    assert!(
        String::from_utf8_lossy(&alias_uninstall.stderr).contains("unsaved"),
        "ordinary alias must reach its own dirty daemon, not be rejected as a different home"
    );
    assert_eq!(
        fs::read(a.join("mods/herdr-nvim/.herdr-nvim")).unwrap(),
        ownership
    );
    // A rename keeps directory identity. This is a discriminating real-filesystem
    // fallback when this OS refuses creation of a case-sensitive directory.
    let moved = root.join("renamed actual A");
    let entry_bytes = fs::read(&ae).unwrap();
    let sidecar_bytes = fs::read(ae.with_extension("app.json")).unwrap();
    let stamp_path = ae.parent().unwrap().join("runtime-stamp");
    let stamp_bytes = fs::read(&stamp_path).unwrap();
    fs::rename(&a, &moved).unwrap();
    let missing_log = root.join("missing-home-commands.log");
    let missing = Command::new(&ae)
        .args(["daemons", "stop", "--all"])
        .env("HN_COMMAND_LOG", &missing_log)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr)
        .contains("app home must be an existing absolute directory"));
    assert!(!missing_log.exists());
    assert!(nvim.child.try_wait().unwrap().is_none());
    let moved_entry = entry(&root, &moved, "home-renamed", &core_a, &fake);
    assert_eq!(
        watch(&moved_entry, &root, "same"),
        state,
        "same actual directory changed instance after rename"
    );
    fs::create_dir(&a).unwrap();
    // Ordinary backup/restore: the original immutable entry AND sidecar remain
    // byte-for-byte unchanged while their recorded path names a new directory.
    for (relative, bytes) in snapshot(&moved) {
        let path = a.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let a_snapshot = snapshot(&a);
    let m_snapshot = snapshot(&moved);
    let registry_snapshot = snapshot(&root.join("XDG_RUNTIME_DIR"));
    let state_snapshot = snapshot(&root.join("XDG_STATE_HOME"));
    let unchanged_log = root.join("unchanged-entry-commands.log");
    for args in [
        vec!["app-current", "--ctx", &ctx],
        vec!["daemons", "stop", "--all"],
        vec!["fs", "{\"op\":\"defaults\"}", "--ctx", &ctx],
        vec!["app-setup", "uninstall"],
        vec!["app-home-check", a.to_str().unwrap()],
    ] {
        let out = Command::new(&ae)
            .args(&args)
            .env("HN_COMMAND_LOG", &unchanged_log)
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "unchanged entry silently rebound: {args:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("app home directory identity changed"),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    env::set_var("HN_COMMAND_LOG", &unchanged_log);
    for upgrade in [false, true] {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-app/install.ps1"))
            .arg("-AppHome")
            .arg(&a)
            .arg("-AppBin")
            .arg(&fake);
        if upgrade {
            cmd.arg("-Binary")
                .arg(env!("CARGO_BIN_EXE_herdr-nvim"))
                .arg("-RuntimeBase")
                .arg(root.join("replacement-unused-runtime"));
        } else {
            cmd.arg("-Uninstall");
        }
        let out = cmd.output().unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("different or unverifiable AppHome"));
    }
    env::remove_var("HN_COMMAND_LOG");
    assert!(
        !unchanged_log.exists(),
        "changed identity reached app/core/spawn/stop/setup"
    );
    assert!(!root.join("replacement-unused-runtime").exists());
    assert_eq!(fs::read(&ae).unwrap(), entry_bytes);
    assert_eq!(
        fs::read(ae.with_extension("app.json")).unwrap(),
        sidecar_bytes
    );
    assert_eq!(fs::read(&stamp_path).unwrap(), stamp_bytes);
    assert_eq!(snapshot(&a), a_snapshot);
    assert_eq!(snapshot(&moved), m_snapshot);
    assert_eq!(snapshot(&root.join("XDG_RUNTIME_DIR")), registry_snapshot);
    assert_eq!(snapshot(&root.join("XDG_STATE_HOME")), state_snapshot);
    assert!(nvim.child.try_wait().unwrap().is_none());
    assert_eq!(nvim.eval("getpid()"), nvim.child.id().to_string());
    assert_eq!(nvim.eval("getline(1)"), "private unsaved");
    assert_eq!(
        nvim.eval("luaeval(\"package.loaded['herdr-nvim.comments'].list()[1].text\")"),
        "pending private review"
    );
    assert_eq!(fs::read(&marker).unwrap(), before);
    assert_eq!(fs::read(&state).unwrap(), contents);
    println!("PASS unchanged entry rejects replacement before current/stop/fs/setup/home-check/install/uninstall; snapshots and dirty Child preserved");
    let replacement = entry(&root, &a, "home-replacement", &for_home(&core_a, &a), &fake);
    let (mut replacement_nvim, replacement_marker, replacement_state) =
        daemon(&root, &replacement, "same");
    assert_ne!(
        replacement_state, state,
        "two actual directories merged by path spelling"
    );
    assert_ne!(replacement_marker, marker);
    assert_ne!(replacement_nvim.pipe, nvim.pipe);
    owned(&a, &moved_entry);
    let replacement_before = snapshot(&a);
    let moved_before = snapshot(&moved);
    let identity_log = root.join("identity-commands.log");
    env::set_var("HN_COMMAND_LOG", &identity_log);
    let wrong = uninstall(&a, &fake);
    env::remove_var("HN_COMMAND_LOG");
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("different or unverifiable AppHome"));
    assert!(
        !identity_log.exists(),
        "wrong home reached status/core/stop/setup"
    );
    assert_eq!(snapshot(&a), replacement_before);
    assert_eq!(snapshot(&moved), moved_before);
    assert!(nvim.child.try_wait().unwrap().is_none());
    assert!(replacement_nvim.child.try_wait().unwrap().is_none());
    assert_eq!(replacement_nvim.eval("getline(1)"), "private unsaved");
    assert!(replacement_marker.exists() && replacement_state.exists());
    drop(replacement_nvim);
    fs::remove_dir_all(&a).unwrap();
    fs::rename(&moved, &a).unwrap();
    assert_eq!(watch(&ae, &root, "same"), state);
    ok(run(&ae, &["fs", "{\"op\":\"defaults\"}", "--ctx", &ctx]));
    assert_eq!(nvim.eval("getpid()"), nvim.child.id().to_string());
    assert_eq!(nvim.eval("getline(1)"), "private unsaved");
    assert_eq!(
        nvim.eval("luaeval(\"package.loaded['herdr-nvim.comments'].list()[1].text\")"),
        "pending private review"
    );
    assert_eq!(fs::read(&ae).unwrap(), entry_bytes);
    assert_eq!(
        fs::read(ae.with_extension("app.json")).unwrap(),
        sidecar_bytes
    );
    println!("PASS original unchanged entry restored: same state/PID/dirty/comment; case alias, explicit renamed entry and distinct fresh replacement also verified");
    case_sensitive_homes(&root, &core_a, &fake);
    // Restored B marker points at actual A native entry. No stop/setup/delete.
    owned(&b, &ae);
    let akeys = fs::read(a.join("keys.json")).unwrap();
    let bkeys = fs::read(b.join("keys.json")).unwrap();
    let rejected = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-app/install.ps1"))
        .arg("-AppHome")
        .arg(&b)
        .arg("-AppBin")
        .arg(&fake)
        .arg("-Uninstall")
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("different or unverifiable AppHome"));
    assert_eq!(fs::read(a.join("keys.json")).unwrap(), akeys);
    assert_eq!(fs::read(b.join("keys.json")).unwrap(), bkeys);
    assert!(b.join("mods/herdr-nvim/.herdr-nvim").exists());
    assert!(nvim.child.try_wait().unwrap().is_none());
    assert_eq!(fs::read(&marker).unwrap(), before);
    explicit_storage_overrides(&root, &ae);
    incomplete_binding_refusal(&root, &a, &core_a, &fake, &mut nvim, &marker, &state);
    legacy_binding_refusal(&root, &a, &core_a, &fake, &mut nvim, &marker, &state);
    assert!(nvim.child.try_wait().unwrap().is_none());
    assert_eq!(fs::read(&marker).unwrap(), before);
    assert_eq!(fs::read(&state).unwrap(), contents);
    // Missing config becoming present is fail-closed, not a new namespace.
    let config = PathBuf::from(core_a["config_path"].as_str().unwrap());
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "socket = 'different'\n").unwrap();
    let rejected = run(&ae, &["app-watch"]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("core config changed"));
    assert!(nvim.child.try_wait().unwrap().is_none());
    fs::remove_file(config).unwrap();
    // Legacy marker in old directory still blocks enumeration/starting work.
    let legacy = root.join("XDG_RUNTIME_DIR/herdr-nvim/app/old.pipe");
    fs::write(&legacy, b"old").unwrap();
    let rejected = run(&ae, &["app-watch"]);
    assert!(!rejected.status.success());
    assert!(legacy.exists());
    assert!(nvim.child.try_wait().unwrap().is_none());
    drop(bnvim);
    drop(other);
    drop(nvim);
    fs::remove_dir_all(&root).unwrap();
}

fn case_sensitive_homes(root: &Path, core: &Value, fake: &Path) {
    let parent = root.join("private-case-sensitive");
    fs::create_dir(&parent).unwrap();
    for args in [
        vec!["queryCaseSensitiveInfo"],
        vec!["setCaseSensitiveInfo", "enable"],
        vec!["queryCaseSensitiveInfo"],
    ] {
        let mut cmd = Command::new("fsutil.exe");
        cmd.args(["file", args[0]]).arg(&parent);
        if args.len() > 1 {
            cmd.arg(args[1]);
        }
        let out = cmd.output().unwrap();
        println!(
            "fsutil {args:?}: {} {} {}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let a = parent.join("Home");
    let b = parent.join("home");
    fs::create_dir(&a).unwrap();
    match fs::create_dir(&b) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            println!("NOT RUN: OS did not permit a private case-sensitive directory; actual identity rename/replacement and ordinary case-alias regressions ran instead");
            return;
        }
        Err(e) => panic!("case fixture: {e}"),
    }
    fs::write(a.join("keys.json"), "{\"keep\":\"A\"}").unwrap();
    fs::write(b.join("keys.json"), "{\"keep\":\"B\"}").unwrap();
    let ae = entry(root, &a, "case-A-entry", &for_home(core, &a), fake);
    let be = entry(root, &b, "case-B-entry", &for_home(core, &b), fake);
    let (mut an, am, ast) = daemon(root, &ae, "case");
    let (mut bn, bm, bst) = daemon(root, &be, "case");
    assert_ne!(ast, bst);
    assert_ne!(am, bm);
    assert_ne!(an.pipe, bn.pipe);
    owned(&b, &ae);
    let before = snapshot(&b);
    let a_before = snapshot(&a);
    let rejected = uninstall(&b, fake);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("different or unverifiable AppHome"));
    assert_eq!(snapshot(&a), a_before);
    assert_eq!(snapshot(&b), before);
    assert!(an.child.try_wait().unwrap().is_none() && bn.child.try_wait().unwrap().is_none());
    for n in [&an, &bn] {
        assert_eq!(n.eval("getline(1)"), "private unsaved");
    }
    for p in [&am, &bm, &ast, &bst] {
        assert!(p.exists());
    }
    println!("PASS actual case-sensitive homes: distinct state/registry/pipe; wrong-home uninstall refused");
}
fn uninstall(home: &Path, fake: &Path) -> Output {
    Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-app/install.ps1"))
        .arg("-AppHome")
        .arg(home)
        .arg("-AppBin")
        .arg(fake)
        .arg("-Uninstall")
        .output()
        .unwrap()
}
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(base: &Path, path: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.push((
                    path.strip_prefix(base).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn incomplete_binding_refusal(
    root: &Path,
    home: &Path,
    core: &Value,
    fake: &Path,
    nvim: &mut Nvim,
    marker: &Path,
    state: &Path,
) {
    let log = root.join("incomplete-binding-commands.log");
    let marker_before = fs::read(marker).unwrap();
    let state_before = fs::read(state).unwrap();
    for (kind, names) in [
        ("runtime", vec!["XDG_RUNTIME_DIR", "TMP", "TEMP"]),
        ("state", vec!["XDG_STATE_HOME", "HOME", "USERPROFILE"]),
    ] {
        let mut capture = Command::new(env!("CARGO_BIN_EXE_herdr-nvim"));
        capture.env("HERDR_APP_HOME", home);
        for name in &names {
            capture.env_remove(name);
        }
        let out = capture.arg("app-core-binding").output().unwrap();
        assert!(
            !out.status.success(),
            "{kind} base capture must fail closed, got {}",
            String::from_utf8_lossy(&out.stdout)
        );
        let reason = format!("bound {kind} base unavailable");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&reason),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut incomplete = core.clone();
        for name in &names {
            incomplete["selectors"][name] = Value::Null;
        }
        let bad = entry(root, home, &format!("incomplete-{kind}"), &incomplete, fake);
        owned(home, &bad);
        let home_before = snapshot(home);
        let ctx = json!({"space":"sp","tab":"same","cwd":root}).to_string();
        // Same immutable entry, changing ONLY caller XDG/TEMP must not rescue
        // an incomplete binding or silently redirect registry/state.
        for changed_caller in [false, true] {
            for args in [
                vec!["app-current", "--ctx", &ctx],
                vec!["daemons", "stop", "--all"],
                vec!["app-setup", "uninstall"],
                vec!["fs", "{\"op\":\"defaults\"}", "--ctx", &ctx],
            ] {
                let mut cmd = Command::new(&bad);
                cmd.args(args).env("HN_COMMAND_LOG", &log);
                if changed_caller {
                    if kind == "runtime" {
                        cmd.env("XDG_RUNTIME_DIR", root.join("caller-new-runtime"))
                            .env("TEMP", root.join("caller-new-temp"));
                    } else {
                        cmd.env("XDG_STATE_HOME", root.join("caller-new-state"));
                    }
                }
                let out = cmd.output().unwrap();
                assert!(!out.status.success());
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains(&reason),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                assert!(
                    !log.exists(),
                    "invalid entry called app/core before rejecting"
                );
                assert_eq!(snapshot(home), home_before);
                assert_eq!(fs::read(marker).unwrap(), marker_before);
                assert_eq!(fs::read(state).unwrap(), state_before);
                assert!(nvim.child.try_wait().unwrap().is_none());
                assert_eq!(nvim.eval("getpid()"), nvim.child.id().to_string());
                assert_eq!(nvim.eval("getline(1)"), "private unsaved");
                assert_eq!(
                    nvim.eval("luaeval(\"package.loaded['herdr-nvim.comments'].list()[1].text\")"),
                    "pending private review"
                );
            }
        }
        env::set_var("HN_COMMAND_LOG", &log);
        let rejected = uninstall(home, fake);
        env::remove_var("HN_COMMAND_LOG");
        assert!(!rejected.status.success());
        assert!(!log.exists());
        assert_eq!(snapshot(home), home_before);
        assert!(!root.join("caller-new-runtime").exists());
        assert!(!root.join("caller-new-temp").exists());
        assert!(!root.join("caller-new-state").exists());
        println!("PASS incomplete {kind}: capture, native entry operations and uninstall refuse before app/core/spawn/stop/setup/delete; dirty work preserved");
    }
}

fn legacy_binding_refusal(
    root: &Path,
    home: &Path,
    core: &Value,
    fake: &Path,
    nvim: &mut Nvim,
    marker: &Path,
    state: &Path,
) {
    let log = root.join("legacy-commands.log");
    let marker_before = fs::read(marker).unwrap();
    let state_before = fs::read(state).unwrap();
    let registry_before = snapshot(&root.join("XDG_RUNTIME_DIR"));
    let ctx = json!({"space":"sp","tab":"same","cwd":root}).to_string();
    for kind in [
        "v1",
        "v2",
        "unknown-version",
        "missing-identity",
        "missing-volume",
        "bad-id",
        "zero-id",
        "missing-config-fingerprint",
        "unknown-field",
    ] {
        let mut bad = core.clone();
        match kind {
            "v1" | "v2" => {
                bad["version"] = json!(if kind == "v1" { 1 } else { 2 });
                bad.as_object_mut().unwrap().remove("app_home_identity");
            }
            "unknown-version" => bad["version"] = json!(core["version"].as_u64().unwrap() + 1),
            "missing-identity" => {
                bad.as_object_mut().unwrap().remove("app_home_identity");
            }
            "missing-volume" => {
                bad["app_home_identity"]
                    .as_object_mut()
                    .unwrap()
                    .remove("volume_serial");
            }
            "bad-id" => bad["app_home_identity"]["directory_id"] = json!("not-a-file-id"),
            "zero-id" => bad["app_home_identity"]["directory_id"] = json!("0".repeat(32)),
            "missing-config-fingerprint" => {
                bad.as_object_mut().unwrap().remove("config_fingerprint");
            }
            "unknown-field" => bad["unexpected"] = json!("no"),
            _ => unreachable!(),
        }
        let old = entry(root, home, &format!("invalid-{kind}"), &bad, fake);
        owned(home, &old);
        let before = snapshot(home);
        for args in [
            vec!["app-current", "--ctx", &ctx],
            vec!["daemons", "stop", "--all"],
            vec!["app-setup", "uninstall"],
            vec!["fs", "{\"op\":\"defaults\"}", "--ctx", &ctx],
        ] {
            let rejected = Command::new(&old)
                .args(&args)
                .env("HN_COMMAND_LOG", &log)
                .output()
                .unwrap();
            assert!(!rejected.status.success(), "{kind} {args:?}");
            let reason = String::from_utf8_lossy(&rejected.stderr);
            assert!(reason.contains("invalid or legacy"), "{kind}: {reason}");
        }
        env::set_var("HN_COMMAND_LOG", &log);
        for upgrade in [false, true] {
            let mut cmd = Command::new("powershell.exe");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-app/install.ps1"))
                .arg("-AppHome")
                .arg(home)
                .arg("-AppBin")
                .arg(fake);
            if upgrade {
                cmd.arg("-Binary")
                    .arg(env!("CARGO_BIN_EXE_herdr-nvim"))
                    .arg("-RuntimeBase")
                    .arg(root.join("legacy-unused-runtime"));
            } else {
                cmd.arg("-Uninstall");
            }
            let rejected = cmd.output().unwrap();
            assert!(!rejected.status.success());
            assert!(String::from_utf8_lossy(&rejected.stderr)
                .contains("Installed entry has a legacy/unverifiable core binding"));
        }
        env::remove_var("HN_COMMAND_LOG");
        assert!(!log.exists());
        assert!(!root.join("legacy-unused-runtime").exists());
        assert_eq!(snapshot(home), before);
        assert_eq!(snapshot(&root.join("XDG_RUNTIME_DIR")), registry_before);
        assert_eq!(fs::read(marker).unwrap(), marker_before);
        assert_eq!(fs::read(state).unwrap(), state_before);
        assert!(nvim.child.try_wait().unwrap().is_none());
        assert_eq!(nvim.eval("getpid()"), nvim.child.id().to_string());
        assert_eq!(nvim.eval("getline(1)"), "private unsaved");
        assert_eq!(
            nvim.eval("luaeval(\"package.loaded['herdr-nvim.comments'].list()[1].text\")"),
            "pending private review"
        );
        println!("PASS installed {kind}: native operations and installer upgrade/uninstall reject without migration/actions; dirty work preserved");
    }
}

fn explicit_storage_overrides(root: &Path, entry: &Path) {
    let state_base = root.join("explicit-state");
    let ctx = json!({"space":"sp","tab":"same","cwd":root}).to_string();
    let out = ok(Command::new(entry)
        .args(["app-current", "--ctx", &ctx])
        .env("HERDR_NVIM_STATE_DIR", &state_base)
        .output()
        .unwrap());
    let current: Value = serde_json::from_str(&out).unwrap();
    assert!(Path::new(current["watch"].as_str().unwrap()).starts_with(&state_base));
    let runtime_base = root.join("explicit-runtime");
    fs::create_dir_all(runtime_base.join("app")).unwrap();
    let legacy = runtime_base.join("app/old.pipe");
    fs::write(&legacy, b"private override sentinel").unwrap();
    let rejected = Command::new(entry)
        .args(["daemons", "--json"])
        .env("HERDR_NVIM_RUNTIME_DIR", &runtime_base)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("unscoped legacy/v2 marker"));
    assert_eq!(fs::read(&legacy).unwrap(), b"private override sentinel");
    println!("PASS explicit runtime/state overrides still take precedence for valid binding");
}
