//! Input validation for container identifiers and volume mounts.
//!
//! Both of these values flow into **host** filesystem paths:
//!
//! ```text
//! volumes_path.join(&volume_id)                -> bind-mounted read/write
//! base_path.join("containers").join(internal_id) -> entrypoint.sh written here
//! ```
//!
//! Neither was validated before this module existed, so a `volume_id` of
//! `../../../../etc` escaped the storage root and was bind-mounted into the
//! container, and an `internal_id` of `../../../../etc/cron.d` wrote
//! attacker-controlled `entrypoint.sh` content outside the storage root.

use std::path::{Component, Path, PathBuf};

/// Maximum length for an identifier. Generous for UUIDs and slugs, short enough
/// to keep path construction and docker container names sane.
const MAX_ID_LEN: usize = 128;

/// Reject anything that could traverse, nest, or denote an absolute path.
///
/// Callers join these values onto a base directory, so the only safe shape is a
/// single, plain path component. Rejecting `..`, separators, and null bytes
/// removes every form of traversal without needing to canonicalize.
pub fn validate_identifier(kind: &str, id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err(format!("{} cannot be empty", kind));
    }

    if id.len() > MAX_ID_LEN {
        return Err(format!(
            "{} is too long (max {} characters)",
            kind, MAX_ID_LEN
        ));
    }

    // Null bytes truncate paths in the C layer beneath std::fs.
    if id.contains('\0') {
        return Err(format!("{} contains a null byte", kind));
    }

    // Separators (both flavours, since the daemon is cross-platform), traversal
    // segments, and alternate data stream / drive syntax.
    if id.contains('/') || id.contains('\\') || id.contains(':') || id.contains("..") {
        return Err(format!(
            "{} may not contain path separators or '..'",
            kind
        ));
    }

    // Leading dot would create hidden dirs and can still complicate globbing.
    if id.starts_with('.') {
        return Err(format!("{} may not start with a dot", kind));
    }

    // Whitespace and control characters have no place in an identifier and are a
    // common vector for confusing logs and shell-adjacent consumers.
    if id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("{} may not contain whitespace or control characters", kind));
    }

    // Defence in depth: parse it and require exactly one Normal component. If a
    // future refactor loosens the checks above, this still holds the line.
    let mut components = Path::new(id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(format!("{} must be a single plain path component", kind)),
    }
}

/// Validate a mount map supplied by the API: container target -> host source.
///
/// The previous implementation checked only the **target** against a small
/// denylist and merely required the source to be non-empty. That left the source
/// completely unconstrained, so `{"/x": "/var/run/docker.sock"}` passed and the
/// container ran as root with the host docker socket mounted read/write — a
/// complete host compromise. `/x` was used as the target precisely because it is
/// absent from the denylist.
///
/// Sources are now confined to the daemon's own storage root, which is the only
/// place container data is supposed to live.
pub fn validate_mounts(
    storage_root: &Path,
    volumes: &std::collections::HashMap<String, String>,
) -> Result<(), String> {
    for (target, source) in volumes {
        validate_mount_target(target)?;
        validate_mount_source(storage_root, target, source)?;
    }

    Ok(())
}

/// The container-side path. Must be absolute and outside system directories.
fn validate_mount_target(target: &str) -> Result<(), String> {
    if target.is_empty() {
        return Err("Mount target cannot be empty".to_string());
    }

    if !target.starts_with('/') {
        return Err(format!("Mount target must be an absolute path: {}", target));
    }

    if target.contains('\0') {
        return Err("Mount target contains a null byte".to_string());
    }

    // Collapse trailing slashes so "/etc/" and "/etc" are treated identically —
    // otherwise "/etc/" walks straight past a prefix comparison.
    let normalized = target.trim_end_matches('/');

    // "/" or "///": mounting over the container root would shadow the entire
    // filesystem, including the paths the daemon itself mounts below it.
    if normalized.is_empty() {
        return Err(format!("Cannot mount over the container root: {}", target));
    }

    // Container-side traversal can escape the intended mount point.
    if normalized.contains("..") {
        return Err(format!("Mount target may not contain '..': {}", target));
    }

    // Children of these are as dangerous as the directories themselves:
    // mounting at /etc/cron.d or /root/.ssh is equivalent to mounting /etc.
    const BLOCKED_PREFIXES: [&str; 13] = [
        "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/sbin", "/sys",
        "/usr", "/var", "/run",
    ];

    for blocked in BLOCKED_PREFIXES {
        if normalized == blocked || normalized.starts_with(&format!("{}/", blocked)) {
            return Err(format!("Cannot mount to system path: {}", target));
        }
    }

    Ok(())
}

/// The host-side path. Must resolve inside the storage root.
fn validate_mount_source(storage_root: &Path, target: &str, source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Err(format!("Invalid source path for target: {}", target));
    }

    if source.contains('\0') {
        return Err(format!("Mount source contains a null byte: {}", target));
    }

    // Required: bare directory names like "backups" are resolved relative to the
    // daemon's working directory, which is not a path we can validate.
    let source_path = PathBuf::from(source);
    if !source_path.is_absolute() {
        return Err(format!(
            "Mount source must be an absolute path inside the storage root: {}",
            source
        ));
    }

    // Try to resolve symlinks. A nonexistent path cannot be canonicalized, so fall
    // back to the parent and treat the missing leaf as unresolvable-but-local.
    let resolved = canonicalize_parent_of(&source_path);
    let resolved = match resolved {
        Some(path) => path,
        None => {
            return Err(format!(
                "Mount source must exist inside the storage root: {}",
                source
            ))
        }
    };

    let root = match storage_root.canonicalize() {
        Ok(root) => root,
        Err(e) => {
            // Failing closed is the right call: without a canonical root we cannot
            // prove containment, and this is a security boundary.
            return Err(format!("Cannot resolve storage root: {}", e));
        }
    };

    if !resolved.starts_with(&root) {
        return Err(format!(
            "Mount source escapes the storage root and is not allowed: {}",
            source
        ));
    }

    Ok(())
}

/// Canonicalize `path`, or its nearest existing ancestor if the leaf is missing.
///
/// Resolving the ancestor is enough to prove containment: if the deepest existing
/// directory is inside the root, anything created beneath it is too.
fn canonicalize_parent_of(path: &Path) -> Option<PathBuf> {
    if let Ok(resolved) = path.canonicalize() {
        return Some(resolved);
    }

    let mut ancestor = path.parent();
    while let Some(current) = ancestor {
        if let Ok(resolved) = current.canonicalize() {
            return Some(resolved);
        }
        ancestor = current.parent();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn rejects_traversal_identifiers() {
        for id in [
            "../etc",
            "../../../../etc",
            "foo/../../etc",
            "/etc/passwd",
            "a/b",
            "a\\b",
            "..",
            "C:evil",
            ".hidden",
            "with space",
            "with\0null",
            "",
        ] {
            assert!(
                validate_identifier("internal_id", id).is_err(),
                "expected {:?} to be rejected",
                id
            );
        }
    }

    #[test]
    fn accepts_typical_identifiers() {
        for id in [
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "my-server",
            "server_01",
            "Server01",
        ] {
            assert!(
                validate_identifier("internal_id", id).is_ok(),
                "expected {:?} to be accepted",
                id
            );
        }
    }

    #[test]
    fn rejects_mount_source_outside_storage_root() {
        let root = std::env::temp_dir().join("lightd_validation_test");
        std::fs::create_dir_all(&root).unwrap();

        let mut volumes = HashMap::new();
        // The exact payload from the review: legal target, hostile source.
        volumes.insert("/x".to_string(), "/var/run/docker.sock".to_string());
        assert!(validate_mounts(&root, &volumes).is_err());

        volumes.clear();
        volumes.insert("/x".to_string(), "/".to_string());
        assert!(validate_mounts(&root, &volumes).is_err());

        volumes.clear();
        volumes.insert("/x".to_string(), "/etc/passwd".to_string());
        assert!(validate_mounts(&root, &volumes).is_err());

        // Relative sources are not resolvable against a known root.
        volumes.clear();
        volumes.insert("/x".to_string(), "somewhere".to_string());
        assert!(validate_mounts(&root, &volumes).is_err());
    }

    #[test]
    fn accepts_mount_source_inside_storage_root() {
        let root = std::env::temp_dir().join("lightd_validation_test_ok");
        let inside = root.join("backups");
        std::fs::create_dir_all(&inside).unwrap();

        let mut volumes = HashMap::new();
        volumes.insert(
            "/data".to_string(),
            inside.to_string_lossy().to_string(),
        );
        assert!(validate_mounts(&root, &volumes).is_ok());
    }

    #[test]
    fn rejects_symlink_escaping_storage_root() {
        let root = std::env::temp_dir().join("lightd_validation_test_symlink");
        std::fs::create_dir_all(&root).unwrap();

        // A symlink inside the root pointing outside it must not be accepted,
        // because canonicalization resolves it to an outside path.
        #[cfg(unix)]
        {
            let link = root.join("escape");
            let _ = std::fs::remove_file(&link);
            if std::os::unix::fs::symlink("/etc", &link).is_ok() {
                let mut volumes = HashMap::new();
                volumes.insert("/x".to_string(), link.to_string_lossy().to_string());
                assert!(
                    validate_mounts(&root, &volumes).is_err(),
                    "symlink escape must be rejected"
                );
            }
        }
    }

    #[test]
    fn rejects_system_mount_targets() {
        let root = std::env::temp_dir();
        let inside = root.to_string_lossy().to_string();

        for target in ["/etc", "/etc/cron.d", "/root/.ssh", "/", "/proc", "/dev", "relative"] {
            let mut volumes = HashMap::new();
            volumes.insert(target.to_string(), inside.clone());
            assert!(
                validate_mounts(&root, &volumes).is_err(),
                "expected target {:?} to be rejected",
                target
            );
        }
    }

    #[test]
    fn rejects_trailing_slash_and_traversal_targets() {
        let root = std::env::temp_dir();
        let inside = root.to_string_lossy().to_string();

        // "/etc/" must not slip past a prefix comparison, "/" and "///" must not
        // shadow the container root, and ".." must not escape the mount point.
        for target in ["/etc/", "/etc/", "///", "/foo/../etc", "/data/../etc"] {
            let mut volumes = HashMap::new();
            volumes.insert(target.to_string(), inside.clone());
            assert!(
                validate_mounts(&root, &volumes).is_err(),
                "expected target {:?} to be rejected",
                target
            );
        }
    }
}