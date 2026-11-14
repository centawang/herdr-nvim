//! Stable pipe naming shared with the integration harness. Not a security
//! token: Windows still controls access. Different users, runtime directories
//! and hosts must never discover/stop each other's tab with the same key.
use std::path::{Path, PathBuf};

pub(crate) fn pipe(dir: &Path, key: &str) -> PathBuf {
    let user = format!(
        "{}\\{}:{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default(),
        std::env::var("USERPROFILE").unwrap_or_default()
    );
    let absolute = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("runtime needs a working directory")
            .join(dir)
    };
    // Fixed FNV-1a, not DefaultHasher (whose implementation is unspecified).
    let identity = format!(
        "{user}\0{}",
        absolute.to_string_lossy().replace('/', "\\").to_lowercase()
    );
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    PathBuf::from(format!(r"\\.\pipe\herdr-nvim-v2-{hash:016x}-{key}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolates_runtime_and_host() {
        let root = std::env::temp_dir().join("namespace-test");
        assert_eq!(pipe(&root, "w_t"), pipe(&root, "w_t"));
        assert_ne!(pipe(&root, "w_t"), pipe(&root.join("app"), "w_t"));
        assert_ne!(
            pipe(&root, "w_t"),
            pipe(&root.with_extension("other"), "w_t")
        );
    }
}
