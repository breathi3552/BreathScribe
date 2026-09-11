//! 将旧版 Handy Cloud 用户数据迁移到 BreathScribe，不覆盖目标目录已有数据。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Marker file name placed in target data directory once migration succeeds.
pub const MIGRATION_MARKER_FILE: &str = ".migrated_from_handycloud";

/// Legacy bundle identifier for Handy-Cloud.
pub const LEGACY_BUNDLE_IDENTIFIER: &str = "io.github.breathi3552.handycloud";

/// New bundle identifier for BreathScribe.
pub const NEW_BUNDLE_IDENTIFIER: &str = "io.github.breathi3552.breathscribe";

/// Summary report of data migration operations.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Whether migration was performed
    pub migrated: bool,
    /// Reason if migration was skipped
    pub skip_reason: Option<String>,
    /// Source legacy directory
    pub source_dir: Option<PathBuf>,
    /// Target directory
    pub target_dir: PathBuf,
    /// Number of regular files copied (settings, db, etc.)
    pub files_copied: usize,
    /// Number of model files hard-linked
    pub models_hard_linked: usize,
    /// Number of model files copied (fallback when hard link failed)
    pub models_copied_fallback: usize,
    /// Number of recording files hard-linked or copied
    pub recordings_migrated: usize,
    /// Total bytes copied or linked
    pub bytes_migrated: u64,
}

/// Automatically detect legacy directory and migrate data to `target_dir` if needed.
///
/// If `explicit_legacy_dir` is `Some`, that path is checked first. Otherwise,
/// it probes for the legacy identifier under `target_dir.parent()`.
pub fn migrate_if_needed(
    target_dir: &Path,
    explicit_legacy_dir: Option<&Path>,
) -> io::Result<MigrationReport> {
    migrate_with_linker(target_dir, explicit_legacy_dir, |s, d| fs::hard_link(s, d))
}

/// Internal implementation allowing injection of custom hard link implementation for testing.
pub fn migrate_with_linker<F>(
    target_dir: &Path,
    explicit_legacy_dir: Option<&Path>,
    hard_linker: F,
) -> io::Result<MigrationReport>
where
    F: Fn(&Path, &Path) -> io::Result<()>,
{
    let marker_path = target_dir.join(MIGRATION_MARKER_FILE);
    if marker_path.exists() {
        return Ok(MigrationReport {
            migrated: false,
            skip_reason: Some("Migration marker already exists".to_string()),
            source_dir: None,
            target_dir: target_dir.to_path_buf(),
            ..Default::default()
        });
    }

    let legacy_dir = match explicit_legacy_dir {
        Some(p) => p.to_path_buf(),
        None => match target_dir.parent() {
            Some(parent) => parent.join(LEGACY_BUNDLE_IDENTIFIER),
            None => {
                return Ok(MigrationReport {
                    migrated: false,
                    skip_reason: Some("Could not determine parent directory".to_string()),
                    source_dir: None,
                    target_dir: target_dir.to_path_buf(),
                    ..Default::default()
                });
            }
        },
    };

    if !legacy_dir.exists() {
        return Ok(MigrationReport {
            migrated: false,
            skip_reason: Some(format!(
                "Legacy data directory does not exist: {}",
                legacy_dir.display()
            )),
            source_dir: Some(legacy_dir),
            target_dir: target_dir.to_path_buf(),
            ..Default::default()
        });
    }

    if let (Ok(canon_source), Ok(canon_target)) =
        (legacy_dir.canonicalize(), target_dir.canonicalize())
    {
        if canon_source == canon_target {
            return Ok(MigrationReport {
                migrated: false,
                skip_reason: Some("Source and target directories are identical".to_string()),
                source_dir: Some(legacy_dir),
                target_dir: target_dir.to_path_buf(),
                ..Default::default()
            });
        }
    }

    // Safety: if target directory already contains user settings or history db,
    // don't overwrite user's fresh data in the new version.
    let target_has_user_data = target_dir.join("settings_store.json").exists()
        || target_dir.join("settings.json").exists()
        || target_dir.join("history.db").exists();

    if target_has_user_data {
        // Mark as migrated to prevent checking every startup
        let _ = fs::create_dir_all(target_dir);
        let _ = fs::write(
            &marker_path,
            format!(
                "skipped_due_to_existing_data: true\nlegacy_dir: {}\n",
                legacy_dir.display()
            ),
        );
        return Ok(MigrationReport {
            migrated: false,
            skip_reason: Some("Target directory already has user data".to_string()),
            source_dir: Some(legacy_dir),
            target_dir: target_dir.to_path_buf(),
            ..Default::default()
        });
    }

    fs::create_dir_all(target_dir)?;

    let mut report = MigrationReport {
        migrated: true,
        skip_reason: None,
        source_dir: Some(legacy_dir.clone()),
        target_dir: target_dir.to_path_buf(),
        ..Default::default()
    };

    let direct_copy_files = [
        "settings_store.json",
        "settings.json",
        "history.db",
        "history.db-wal",
        "history.db-shm",
    ];

    for file_name in &direct_copy_files {
        let src_file = legacy_dir.join(file_name);
        let dst_file = target_dir.join(file_name);
        if src_file.is_file() {
            match fs::copy(&src_file, &dst_file) {
                Ok(bytes) => {
                    report.files_copied += 1;
                    report.bytes_migrated += bytes;
                    eprintln!("[migration] copied {} ({} bytes)", file_name, bytes);
                }
                Err(e) => {
                    eprintln!("[migration] failed to copy {}: {}", src_file.display(), e);
                }
            }
        }
    }

    let src_models_dir = legacy_dir.join("models");
    let dst_models_dir = target_dir.join("models");
    if src_models_dir.is_dir() {
        migrate_tree_with_links(
            &src_models_dir,
            &dst_models_dir,
            &hard_linker,
            &mut report.models_hard_linked,
            &mut report.models_copied_fallback,
            &mut report.bytes_migrated,
        )?;
    }

    let src_recordings_dir = legacy_dir.join("recordings");
    let dst_recordings_dir = target_dir.join("recordings");
    if src_recordings_dir.is_dir() {
        let mut dummy_fallback = 0;
        migrate_tree_with_links(
            &src_recordings_dir,
            &dst_recordings_dir,
            &hard_linker,
            &mut report.recordings_migrated,
            &mut dummy_fallback,
            &mut report.bytes_migrated,
        )?;
    }

    let marker_content = format!(
        "migrated_from: {}\nfiles_copied: {}\nmodels_hard_linked: {}\nmodels_copied_fallback: {}\nbytes_migrated: {}\n",
        legacy_dir.display(),
        report.files_copied,
        report.models_hard_linked,
        report.models_copied_fallback,
        report.bytes_migrated
    );
    fs::write(&marker_path, marker_content)?;
    eprintln!(
        "[migration] migration complete: {} files copied, {} models linked, {} models copied fallback",
        report.files_copied, report.models_hard_linked, report.models_copied_fallback
    );

    Ok(report)
}

/// Recursively migrate a directory tree, creating hard links where possible
/// and falling back to copy if hard links fail.
fn migrate_tree_with_links<F>(
    src_dir: &Path,
    dst_dir: &Path,
    hard_linker: &F,
    linked_count: &mut usize,
    copied_fallback_count: &mut usize,
    total_bytes: &mut u64,
) -> io::Result<()>
where
    F: Fn(&Path, &Path) -> io::Result<()>,
{
    if !src_dir.exists() {
        return Ok(());
    }

    fs::create_dir_all(dst_dir)?;

    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst_dir.join(&file_name);

        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            migrate_tree_with_links(
                &src_path,
                &dst_path,
                hard_linker,
                linked_count,
                copied_fallback_count,
                total_bytes,
            )?;
        } else if file_type.is_file() {
            let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);

            match hard_linker(&src_path, &dst_path) {
                Ok(()) => {
                    *linked_count += 1;
                    *total_bytes += file_size;
                    eprintln!(
                        "[migration] hard-linked {} -> {}",
                        src_path.display(),
                        dst_path.display()
                    );
                }
                Err(err) => {
                    eprintln!(
                        "[migration] hard link failed ({}), falling back to copy for {}",
                        err,
                        src_path.display()
                    );
                    match fs::copy(&src_path, &dst_path) {
                        Ok(bytes) => {
                            *copied_fallback_count += 1;
                            *total_bytes += bytes;
                        }
                        Err(copy_err) => {
                            eprintln!(
                                "[migration] fallback copy failed for {}: {}",
                                src_path.display(),
                                copy_err
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            let unique = format!(
                "bs_mig_test_{}_{}_{}",
                name,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_migration_detect_and_hard_link() {
        let base = TempDirGuard::new("detect_link");
        let legacy = base.path.join("legacy");
        let target = base.path.join("target");

        fs::create_dir_all(legacy.join("models/subdir")).unwrap();
        fs::write(legacy.join("settings_store.json"), r#"{"theme":"dark"}"#).unwrap();
        fs::write(legacy.join("history.db"), b"SQLite format 3\0test_data").unwrap();
        fs::write(legacy.join("models/whisper-tiny.bin"), b"MODEL_DATA_12345").unwrap();
        fs::write(
            legacy.join("models/subdir/custom.bin"),
            b"CUSTOM_MODEL_DATA",
        )
        .unwrap();

        let report = migrate_if_needed(&target, Some(&legacy)).unwrap();

        assert!(report.migrated);
        assert!(report.skip_reason.is_none());
        assert_eq!(report.files_copied, 2); // settings_store.json + history.db
        assert_eq!(report.models_hard_linked, 2);
        assert_eq!(report.models_copied_fallback, 0);

        assert_eq!(
            fs::read_to_string(target.join("settings_store.json")).unwrap(),
            r#"{"theme":"dark"}"#
        );
        assert_eq!(
            fs::read(target.join("history.db")).unwrap(),
            b"SQLite format 3\0test_data"
        );
        assert_eq!(
            fs::read(target.join("models/whisper-tiny.bin")).unwrap(),
            b"MODEL_DATA_12345"
        );
        assert_eq!(
            fs::read(target.join("models/subdir/custom.bin")).unwrap(),
            b"CUSTOM_MODEL_DATA"
        );
        assert!(target.join(MIGRATION_MARKER_FILE).exists());

        // Verify hard link behavior: modifying one reflects in the other
        // (on filesystems that support hard links)
        let _ = fs::write(
            target.join("models/whisper-tiny.bin"),
            b"MODIFIED_VIA_TARGET",
        );
        assert_eq!(
            fs::read(legacy.join("models/whisper-tiny.bin")).unwrap(),
            b"MODIFIED_VIA_TARGET"
        );
    }

    #[test]
    fn test_migration_idempotent_skip() {
        let base = TempDirGuard::new("idempotent");
        let legacy = base.path.join("legacy");
        let target = base.path.join("target");

        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("settings_store.json"), r#"{"theme":"light"}"#).unwrap();

        fs::create_dir_all(&target).unwrap();
        fs::write(target.join(MIGRATION_MARKER_FILE), "already_migrated").unwrap();

        let report = migrate_if_needed(&target, Some(&legacy)).unwrap();

        assert!(!report.migrated);
        assert_eq!(
            report.skip_reason.as_deref(),
            Some("Migration marker already exists")
        );
        assert!(!target.join("settings_store.json").exists());
    }

    #[test]
    fn test_migration_fallback_to_copy_on_hard_link_failure() {
        let base = TempDirGuard::new("fallback");
        let legacy = base.path.join("legacy");
        let target = base.path.join("target");

        fs::create_dir_all(legacy.join("models")).unwrap();
        fs::write(legacy.join("models/model.gguf"), b"GGUF_HEADER_BYTES").unwrap();

        // Inject simulated hard link failure (e.g. EXDEV / cross-volume)
        let failing_linker = |_src: &Path, _dst: &Path| -> io::Result<()> {
            Err(io::Error::other("Simulated EXDEV cross-device link error"))
        };

        let report = migrate_with_linker(&target, Some(&legacy), failing_linker).unwrap();

        assert!(report.migrated);
        assert_eq!(report.models_hard_linked, 0);
        assert_eq!(report.models_copied_fallback, 1);

        assert_eq!(
            fs::read(target.join("models/model.gguf")).unwrap(),
            b"GGUF_HEADER_BYTES"
        );
        assert!(target.join(MIGRATION_MARKER_FILE).exists());
    }

    #[test]
    fn test_migration_skipped_when_target_already_has_user_data() {
        let base = TempDirGuard::new("existing_data");
        let legacy = base.path.join("legacy");
        let target = base.path.join("target");

        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("settings_store.json"), r#"{"old":"settings"}"#).unwrap();

        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("settings_store.json"), r#"{"new":"settings"}"#).unwrap();

        let report = migrate_if_needed(&target, Some(&legacy)).unwrap();

        assert!(!report.migrated);
        assert_eq!(
            report.skip_reason.as_deref(),
            Some("Target directory already has user data")
        );
        assert_eq!(
            fs::read_to_string(target.join("settings_store.json")).unwrap(),
            r#"{"new":"settings"}"#
        );
        assert!(target.join(MIGRATION_MARKER_FILE).exists());
    }

    #[test]
    fn test_migration_skipped_when_legacy_not_found() {
        let base = TempDirGuard::new("no_legacy");
        let legacy = base.path.join("nonexistent_legacy");
        let target = base.path.join("target");

        let report = migrate_if_needed(&target, Some(&legacy)).unwrap();

        assert!(!report.migrated);
        assert!(report
            .skip_reason
            .as_ref()
            .unwrap()
            .contains("Legacy data directory does not exist"));
    }

    #[test]
    fn test_migration_preserves_recordings() {
        let base = TempDirGuard::new("recordings");
        let legacy = base.path.join("legacy");
        let target = base.path.join("target");

        fs::create_dir_all(legacy.join("recordings")).unwrap();
        fs::write(legacy.join("recordings/rec_1.wav"), b"WAV_AUDIO_DATA_1").unwrap();
        fs::write(legacy.join("recordings/rec_2.wav"), b"WAV_AUDIO_DATA_2").unwrap();

        let report = migrate_if_needed(&target, Some(&legacy)).unwrap();

        assert!(report.migrated);
        assert_eq!(report.recordings_migrated, 2);
        assert_eq!(
            fs::read(target.join("recordings/rec_1.wav")).unwrap(),
            b"WAV_AUDIO_DATA_1"
        );
        assert_eq!(
            fs::read(target.join("recordings/rec_2.wav")).unwrap(),
            b"WAV_AUDIO_DATA_2"
        );
    }
}
