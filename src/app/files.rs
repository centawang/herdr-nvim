//! Bounded native IO for app mods: external reads, and writes only to our cache.
//! All paths are explicit argv, never interpolated into a shell script.
use std::{
    env, fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use serde_json::json;

fn bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    anyhow::ensure!(file.metadata()?.is_file(), "not a regular file");
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= max,
        "too big to preview (limit {} MiB)",
        max / 1048576
    );
    Ok(bytes)
}

fn link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Junctions and other reparse points are not expandable tree directories.
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

pub(crate) fn files_cmd() -> Result<()> {
    let args: Vec<String> = env::args().skip(2).collect();
    let [op, path] = args.as_slice() else {
        bail!("usage: app-files <list|text|image> <absolute path>")
    };
    let path = Path::new(path);
    anyhow::ensure!(path.is_absolute(), "app-files needs an absolute path");
    match op.as_str() {
        "list" => {
            let mut entries = Vec::new();
            for entry in fs::read_dir(path)?.take(501) {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                let meta = fs::symlink_metadata(entry.path())?;
                entries.push(json!({"name":name,"dir":meta.is_dir() && !link(&meta)}));
                if entries.len() == 500 {
                    break;
                }
            }
            let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
            let ignored = ignored_names(path, &names)?;
            println!("{}", json!({"entries":entries,"ignored":ignored}));
        }
        "text" => println!(
            "{}",
            json!({"text":String::from_utf8(bounded(path, 2 * 1048576)?).context("preview is not UTF-8")?})
        ),
        "image" => println!("{}", json!({"src": cache_image(path)?})),
        _ => bail!("unknown app-files operation"),
    }
    Ok(())
}

// Names are literal filesystem entries, not argv/pathspecs. Write and drain
// concurrently: 500 long names can exceed both Windows argv and pipe buffers.
fn ignored_names(path: &Path, names: &[&str]) -> Result<Vec<String>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let input: Vec<u8> = names
        .iter()
        .flat_map(|name| name.bytes().chain([0]))
        .collect();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["check-ignore", "-z", "--stdin"])
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run git check-ignore")?;
    let mut stdin = child.stdin.take().context("git stdin missing")?;
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .context("failed to read git check-ignore");
    let written = writer
        .join()
        .map_err(|_| anyhow::anyhow!("git input writer panicked"))?;
    let output = output?;
    match output.status.code() {
        Some(0) => {
            written.context("failed to write git check-ignore input")?;
            Ok(String::from_utf8(output.stdout)
                .context("git ignored names are not UTF-8")?
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect())
        }
        Some(1) => {
            written.context("failed to write git check-ignore input")?;
            Ok(Vec::new())
        }
        _ => {
            let error = String::from_utf8_lossy(&output.stderr);
            // Git has no distinct exit code for a non-repository. Pin its locale
            // and accept only the standard discovery miss, not bad config,
            // dubious ownership, an invalid GIT_DIR, IO errors or a missing Git.
            if error
                .starts_with("fatal: not a git repository (or any of the parent directories): .git")
                || error.starts_with("fatal: not a git repository (or any parent up to mount point")
            {
                Ok(Vec::new())
            } else {
                bail!(
                    "git check-ignore failed ({}): {}",
                    output.status,
                    error.trim()
                )
            }
        }
    }
}

// Copy bounded image bytes into the app's own cache without transporting tens
// of MiB through exec/stdout. Only this fixed destination is writable. Never
// traverse cache reparse points or overwrite an existing (possibly open) file.
fn cache_image(path: &Path) -> Result<String> {
    use std::io::Write;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    anyhow::ensure!(
        ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg"].contains(&extension.as_str()),
        "not a preview image"
    );
    let bytes = bounded(path, 30 * 1048576)?;
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    let name = format!("{hash:016x}{:x}.{extension}", bytes.len());
    let home = env::var_os("HERDR_APP_HOME").context("app image cache needs HERDR_APP_HOME")?;
    let mut dir = fs::canonicalize(home)?;
    for component in ["mods", "herdr-nvim", ".cache"] {
        dir.push(component);
        match fs::symlink_metadata(&dir) {
            Ok(meta) => anyhow::ensure!(
                meta.is_dir() && !link(&meta),
                "cache directory is a link or not a directory"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&dir)?,
            Err(e) => return Err(e.into()),
        }
    }
    let destination = dir.join(&name);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
    {
        Ok(mut file) => file.write_all(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(&destination)?;
            anyhow::ensure!(
                !link(&meta) && meta.is_file(),
                "cache entry is not a regular file"
            );
            anyhow::ensure!(
                bounded(&destination, 30 * 1048576)? == bytes,
                "cache collision or incomplete entry; remove this cache entry and retry"
            );
        }
        Err(e) => return Err(e.into()),
    }
    Ok(format!("mods/herdr-nvim/.cache/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reads_reject_overflow_and_directories() {
        let dir = env::temp_dir().join(format!("hn-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("空 格.txt");
        fs::write(&file, b"12345").unwrap();
        assert_eq!(bounded(&file, 5).unwrap(), b"12345");
        assert!(bounded(&file, 4).is_err());
        assert!(bounded(&dir, 4).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
