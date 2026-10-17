//! ZIP metadata is untrusted even when only selected entries are read into memory.
use std::path::Path;

pub(crate) fn validate_entry(name: &str, mode: Option<u32>) -> Result<(), String> {
    // Check both separator conventions on every host, including Windows drive/ADS paths.
    if name.is_empty()
        || name.starts_with(['/', '\\'])
        || name.contains([':', '\0'])
        || name.split(['/', '\\']).any(|part| part == "..")
        || mode.is_some_and(|m| m & 0o170000 == 0o120000)
    {
        return Err(format!("unsafe ZIP entry: {name:?}"));
    }
    Ok(())
}

pub(crate) fn validate_archive<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(), String> {
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?;
        validate_entry(&name, Some(entry.external_attributes() >> 16))?;
        if entry.enclosed_name().is_none() {
            return Err(format!("unsafe ZIP entry: {name:?}"));
        }
    }
    Ok(())
}

/// Reject pre-existing links too, before creating files or changing permissions.
pub(crate) fn reject_symlink_ancestors(path: &Path, root: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "symlink in ZIP destination: {}",
                    ancestor.display()
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        // Parents of the caller's trusted destination can legitimately be system links
        // (for example /var on macOS). Only inspect the destination and its contents.
        if ancestor == root {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(name: &str, symlink: bool) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o6777);
        if symlink {
            writer.add_symlink(name, "../../outside", options).unwrap();
        } else {
            writer.start_file(name, options).unwrap();
            writer.write_all(b"payload").unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn unsafe_metadata_is_rejected_by_full_and_range_readers() {
        for (name, symlink) in [
            ("../escape", false),
            ("sub/../escape", false),
            ("/absolute", false),
            ("C:/absolute", false),
            ("C:relative", false),
            ("\\\\server\\share", false),
            ("sub\\..\\escape", false),
            ("link", true),
        ] {
            let bytes = archive(name, symlink);
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            assert!(validate_archive(&mut zip).is_err(), "{name}");

            use crate::onnx_runtime::remote_zip::{
                find_central_directory, parse_central_directory,
            };
            let (offset, size) = find_central_directory(&bytes).unwrap();
            assert!(
                parse_central_directory(&bytes[offset as usize..(offset + size) as usize]).is_err(),
                "{name}"
            );

            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("input.zip");
            std::fs::write(&input, &bytes).unwrap();
            let output = dir.path().join("out");
            assert!(
                crate::runtime::extract_archive(&input, &output).is_err(),
                "{name}"
            );
            assert!(!output.exists(), "validation must precede all writes");
        }
    }

    #[test]
    fn normal_nested_files_extract_without_privileged_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.zip");
        let mut bytes = archive("bin/tool", false);
        // ZipWriter masks special bits; emulate a malicious producer's central attributes.
        let (offset, _) = crate::onnx_runtime::remote_zip::find_central_directory(&bytes).unwrap();
        let attributes = (0o106777u32 << 16).to_le_bytes();
        bytes[offset as usize + 38..offset as usize + 42].copy_from_slice(&attributes);
        std::fs::write(&input, bytes).unwrap();
        let output = dir.path().join("out");
        crate::runtime::extract_archive(&input, &output).unwrap();
        let file = output.join("bin/tool");
        assert_eq!(std::fs::read(&file).unwrap(), b"payload");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(file).unwrap().permissions().mode() & 0o7777,
                0o777
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn trusted_destination_can_have_a_system_symlink_above_it() {
        let dir = tempfile::tempdir().unwrap();
        let alias = dir.path().join("alias");
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let input = dir.path().join("input.zip");
        std::fs::write(&input, archive("bin/tool", false)).unwrap();
        crate::runtime::extract_archive(&input, &alias.join("out")).unwrap();
        assert_eq!(
            std::fs::read(real.join("out/bin/tool")).unwrap(),
            b"payload"
        );
    }

    #[test]
    fn existing_files_are_not_overwritten_or_chmodded() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.zip");
        std::fs::write(&input, archive("tool", false)).unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();
        std::fs::write(output.join("tool"), b"original").unwrap();
        assert!(crate::runtime::extract_archive(&input, &output).is_err());
        assert_eq!(std::fs::read(output.join("tool")).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn existing_destination_symlinks_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.zip");
        std::fs::write(&input, archive("bin/tool", false)).unwrap();
        let output = dir.path().join("out");
        std::fs::create_dir(&output).unwrap();
        std::os::unix::fs::symlink(outside.path(), output.join("bin")).unwrap();
        assert!(crate::runtime::extract_archive(&input, &output).is_err());
        assert!(!outside.path().join("tool").exists());
    }
}
