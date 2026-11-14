//! A directly executable Windows app entry: the normal binary plus an adjacent
//! immutable sidecar, in a versioned runtime. No shell/file-association required.
#[cfg(windows)]
use anyhow::Context;
use anyhow::Result;

pub(crate) fn configure() -> Result<()> {
    #[cfg(windows)]
    {
        #[derive(serde::Deserialize)]
        struct Entry {
            plugin_root: std::path::PathBuf,
            app_home: std::path::PathBuf,
            app_bin: std::path::PathBuf,
            core: super::instance::Core,
        }
        let exe = std::env::current_exe()?;
        let sidecar = exe.with_extension("app.json");
        let text = match std::fs::read_to_string(&sidecar) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Raw development invocations still get the same instance scope.
                if std::env::var("HERDR_NVIM_HOST").as_deref() == Ok("app") {
                    let home = std::env::var_os("HERDR_APP_HOME")
                        .context("app host needs HERDR_APP_HOME")?;
                    bind(
                        &super::instance::Core::capture()?,
                        std::path::Path::new(&home),
                    )?;
                }
                return Ok(());
            }
            Err(e) => return Err(e).context("cannot read app entry sidecar"),
        };
        let entry: Entry = serde_json::from_str(&text)
            .context("invalid or legacy app entry sidecar; save/quit using the original runtime before manual migration. Nothing stopped or replaced")?;
        for path in [&entry.plugin_root, &entry.app_home, &entry.app_bin] {
            anyhow::ensure!(path.is_absolute(), "app entry paths must be absolute");
        }
        bind(&entry.core, &entry.app_home)?;
        std::env::set_var("HERDR_NVIM_HOST", "app");
        std::env::set_var("HERDR_NVIM_SELF", exe);
        std::env::set_var("HERDR_NVIM_PLUGIN_ROOT", entry.plugin_root);
        std::env::set_var("HERDR_APP_HOME", entry.app_home);
        std::env::set_var("HERDR_APP_BIN", entry.app_bin);
    }
    Ok(())
}

#[cfg(windows)]
fn bind(core: &super::instance::Core, home: &std::path::Path) -> Result<()> {
    core.verify()?;
    core.verify_home(home)?;
    std::env::set_var("HERDR_NVIM_INSTANCE", core.scope()?);
    std::env::set_var("HERDR_NVIM_CORE_BINDING", serde_json::to_string(core)?);
    // Also pin selectors inherited by app commands/descendants. The hc client
    // applies the complete path selector whitelist without changing editor HOME.
    for name in ["HC_SESSION", "HC_SOCKET"] {
        match &core.selectors[name] {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
    Ok(())
}
