//! Windows app/core binding. Immutable entries pin only core selectors, never
//! the caller's whole environment. A changed core config is unknown, not empty.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

// The installer and native test fixture read this same shipped schema. Never
// upgrade metadata by changing its version: older entries must be retired first.
#[derive(Deserialize)]
struct Schema {
    version: u32,
    selectors: Vec<String>,
}
static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../herdr-app/binding-schema.json"))
        .expect("embedded binding schema")
});

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomeIdentity {
    volume_serial: String,
    directory_id: String,
}
impl HomeIdentity {
    fn valid(&self) -> bool {
        let hex = |s: &str, len| {
            s.len() == len
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        hex(&self.volume_serial, 16)
            && hex(&self.directory_id, 32)
            && self.directory_id.bytes().any(|b| b != b'0')
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Core {
    pub version: u32,
    pub hc_bin: PathBuf,
    pub selectors: BTreeMap<String, Option<String>>,
    pub config_path: PathBuf,
    // A missing nullable field is NOT equivalent to an explicitly absent config.
    #[serde(deserialize_with = "Option::deserialize")]
    pub config_fingerprint: Option<String>,
    pub app_home_identity: HomeIdentity,
}

pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

/// One filesystem identity contract for scope AND installer home checks. Path
/// case folding is not directory identity on Windows. FILE_ID_INFO identifies
/// the opened directory on its volume, including case-sensitive NTFS homes.
/// Unsupported filesystems/links fail closed; never substitute a path hash.
fn identity(path: &Path) -> Result<HomeIdentity> {
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    anyhow::ensure!(
        path.is_absolute() && path.is_dir(),
        "app home must be an existing absolute directory"
    );
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor).context("cannot verify app home ancestry")?;
        anyhow::ensure!(
            meta.file_attributes() & 0x400 == 0,
            "linked app home cannot be verified; use the original app home"
        );
    }
    let handle = fs::OpenOptions::new()
        .read(true)
        .access_mode(0)
        .share_mode(7)
        .custom_flags(0x02000000)
        .open(path)
        .context("cannot open app home for directory identity")?;
    #[repr(C)]
    struct FileIdInfo {
        volume: u64,
        id: [u8; 16],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandleEx(
            handle: *mut std::ffi::c_void,
            class: i32,
            info: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let mut info = FileIdInfo {
        volume: 0,
        id: [0; 16],
    };
    // FileIdInfo (18) returns a 128-bit file ID, not the truncated legacy index.
    let result = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            18,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FileIdInfo>() as u32,
        )
    };
    anyhow::ensure!(
        result != 0,
        "cannot verify app home directory identity: {}; nothing stopped or replaced",
        std::io::Error::last_os_error()
    );
    anyhow::ensure!(
        info.id != [0; 16],
        "filesystem supplies no reliable app home identity; nothing stopped or replaced"
    );
    let id: String = info.id.iter().map(|b| format!("{b:02x}")).collect();
    Ok(HomeIdentity {
        volume_serial: format!("{:016x}", info.volume),
        directory_id: id,
    })
}

/// Invoked by the installer through the verified installed entry, before any
/// lock/status/stop/setup/delete. PowerShell does not reimplement equivalence.
pub(crate) fn home_check_cmd() -> Result<()> {
    let target = env::args_os()
        .nth(2)
        .context("app-home-check needs an AppHome")?;
    let bound: Core = serde_json::from_str(
        &env::var("HERDR_NVIM_CORE_BINDING").context("app-home-check needs a bound entry")?,
    )?;
    anyhow::ensure!(identity(Path::new(&target))? == bound.app_home_identity,
        "Installed entry belongs to a different or unverifiable AppHome/core instance; nothing stopped or removed");
    Ok(())
}

fn config_fingerprint(path: &Path) -> Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(fingerprint(&bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("cannot verify core config; nothing was stopped"),
    }
}

impl Core {
    pub(crate) fn capture() -> Result<Self> {
        let selectors = SCHEMA
            .selectors
            .iter()
            .map(|name| (name.to_string(), env::var(name).ok()))
            .collect();
        let bin = env::var_os("HERDR_NVIM_HC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("hc.exe"));
        let bin = if bin.is_absolute() {
            bin
        } else {
            env::split_paths(&env::var_os("PATH").unwrap_or_default())
                .map(|p| p.join(&bin))
                .find(|p| p.is_file())
                .context("hc.exe not found; set HERDR_NVIM_HC to its absolute path")?
        };
        let config_path = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| crate::home::home_dir().map(|p| p.join(".config")))
            .context("cannot determine core config home")?
            .join("herdr-core/config.toml");
        Ok(Self {
            version: SCHEMA.version,
            app_home_identity: identity(Path::new(
                &env::var_os("HERDR_APP_HOME").context("binding capture needs HERDR_APP_HOME")?,
            ))?,
            hc_bin: crate::pathkey::canonical(&bin),
            selectors,
            config_fingerprint: config_fingerprint(&config_path)?,
            config_path,
        })
    }

    pub(crate) fn verify(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == SCHEMA.version
                && self.app_home_identity.valid()
                && self.hc_bin.is_absolute()
                && self.config_path.is_absolute()
                && self.selectors.len() == SCHEMA.selectors.len()
                && SCHEMA.selectors.iter().all(|n| self.selectors.contains_key(n)),
            "invalid or legacy core binding; save/quit old nvims, verify exit and reinstall manually; nothing stopped or replaced"
        );
        for (name, value) in &self.selectors {
            if name != "HC_SESSION" {
                if let Some(value) = value {
                    anyhow::ensure!(Path::new(value).is_absolute(), "core selector {name} must be absolute; relative selectors cannot be bound safely. Nothing was stopped or replaced");
                }
            }
        }
        // Validation and actual storage selection share exactly this resolver.
        // Explicit test overrides do not make incomplete immutable metadata safe.
        self.storage_base(true)?;
        self.storage_base(false)?;
        anyhow::ensure!(config_fingerprint(&self.config_path)? == self.config_fingerprint,
            "core config changed since installation; save and quit old nvims, verify their exit, then migrate/reinstall manually. Nothing was stopped or replaced");
        if self.config_fingerprint.is_some() {
            let text =
                fs::read_to_string(&self.config_path).context("cannot verify core config")?;
            let config: toml::Value =
                toml::from_str(&text).context("cannot parse core config for binding")?;
            if let Some(socket) = config.get("socket").and_then(toml::Value::as_str) {
                anyhow::ensure!(Path::new(socket).is_absolute(), "core config socket must be absolute; relative endpoints cannot be bound safely");
            }
        }
        Ok(())
    }

    fn storage_base(&self, runtime: bool) -> Result<PathBuf> {
        let value = |name: &str| {
            self.selectors
                .get(name)
                .and_then(|v| v.as_ref())
                .map(PathBuf::from)
        };
        let base = if runtime {
            value("XDG_RUNTIME_DIR")
                .or_else(|| value("TMP"))
                .or_else(|| value("TEMP"))
                .map(|p| p.join("herdr-nvim"))
        } else {
            value("XDG_STATE_HOME")
                .map(|p| p.join("herdr-nvim"))
                .or_else(|| {
                    value("HOME")
                        .or_else(|| value("USERPROFILE"))
                        .map(|p| p.join(".local/state/herdr-nvim"))
                })
        };
        base.with_context(|| format!("bound {} base unavailable; capture explicit absolute directory selectors and migrate manually after saving/quitting old nvims. Nothing stopped or replaced", if runtime { "runtime" } else { "state" }))
    }

    pub(crate) fn command(&self) -> Command {
        let mut cmd = Command::new(&self.hc_bin);
        for (name, value) in &self.selectors {
            match value {
                Some(value) => {
                    cmd.env(name, value);
                }
                None => {
                    cmd.env_remove(name);
                }
            }
        }
        cmd
    }

    pub(crate) fn verify_home(&self, home: &Path) -> Result<()> {
        anyhow::ensure!(identity(home)? == self.app_home_identity,
            "app home directory identity changed since installation; restore the original directory at its recorded path or save/quit via the original runtime before manual migration. Nothing stopped or replaced");
        Ok(())
    }

    pub(crate) fn scope(&self) -> Result<String> {
        // Use ONLY the captured identity, already verified by early configure.
        // Never reinterpret an immutable entry using the path's current object.
        // Binary and config contents do not create a new namespace.
        Ok(fingerprint(
            format!(
                "file-id-v2:{}:{}\0{}",
                self.app_home_identity.volume_serial,
                self.app_home_identity.directory_id,
                serde_json::to_string(&self.selectors)?
            )
            .as_bytes(),
        ))
    }
}

pub(crate) fn bind_cmd() -> Result<()> {
    let core = Core::capture()?;
    core.verify()?;
    println!("{}", serde_json::to_string(&core)?);
    Ok(())
}

/// Default storage paths use the installed core's path context, not whichever
/// terminal happens to invoke the entry. Explicit plugin overrides still win.
pub(crate) fn storage_base(runtime: bool) -> Option<PathBuf> {
    if crate::host::choose() != crate::host::Host::App {
        return None;
    }
    // No binding is used by host-switching unit fixtures. Once a binding exists,
    // NEVER return None and thereby opt into registry/state's caller fallback.
    // configure verifies these invariants before commands/threads can run.
    let text = env::var_os("HERDR_NVIM_CORE_BINDING")?;
    let core: Core = serde_json::from_str(text.to_str().expect("validated core binding is UTF-8"))
        .expect("entry must validate core binding before selecting storage");
    Some(
        core.storage_base(runtime)
            .expect("entry must validate bound storage before selecting paths"),
    )
}

pub(crate) fn scope_dir(base: PathBuf) -> PathBuf {
    if crate::host::choose() == crate::host::Host::App {
        // Production configure sets this before any paths/threads are used.
        // Unit fixtures that switch host later retain their explicit sandbox.
        if let Ok(scope) = env::var("HERDR_NVIM_INSTANCE") {
            return base.join(format!("instance-{scope}"));
        }
    }
    base
}
