use std::{
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::Serialize;

const MAX_PACKAGE_ENTRIES: usize = 65_536;
const MAX_PACKAGE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PACKAGE_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MIN_ENTRY_RECORD_SIZE: usize = 12;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageEntry {
    pub full_path: String,
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub magic: String,
    pub header_size: u64,
    pub entries: Vec<PackageEntry>,
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0_u8; 4];
    cursor.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_string_i32_size(cursor: &mut Cursor<&[u8]>, max_length: usize) -> Result<String> {
    let length = read_u32(cursor)? as usize;
    if length > max_length {
        bail!("Entry path length {length} exceeds sanity limit");
    }
    let mut bytes = vec![0_u8; length];
    cursor.read_exact(&mut bytes)?;
    Ok(String::from_utf8(bytes)?)
}

fn normalize_entry_path(entry_path: &str) -> Result<PathBuf> {
    let raw = Path::new(entry_path);
    if raw.as_os_str().is_empty() {
        bail!("Package entry path is empty");
    }

    let mut normalized = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::RootDir | Component::ParentDir | Component::Prefix(_) => {
                bail!("Package entry path escapes extraction root: {entry_path}");
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        bail!("Package entry path is empty after normalization");
    }

    Ok(normalized)
}

pub fn parse_pkg(bytes: &[u8]) -> Result<Package> {
    let mut cursor = Cursor::new(bytes);
    let magic = read_string_i32_size(&mut cursor, 32)?;
    let entry_count = read_u32(&mut cursor)?;
    let entry_count = entry_count as usize;
    if entry_count > MAX_PACKAGE_ENTRIES {
        bail!("Package entry count {entry_count} exceeds sanity limit of {MAX_PACKAGE_ENTRIES}");
    }
    let remaining = bytes.len().saturating_sub(cursor.position() as usize);
    let max_entries_by_size = remaining / MIN_ENTRY_RECORD_SIZE;
    if entry_count > max_entries_by_size {
        bail!(
            "Package entry count {entry_count} exceeds remaining header bytes capacity ({max_entries_by_size})"
        );
    }
    let mut entries = Vec::with_capacity(entry_count);

    for _ in 0..entry_count {
        let full_path = read_string_i32_size(&mut cursor, 1024)?;
        let offset = read_u32(&mut cursor)?;
        let length = read_u32(&mut cursor)?;
        entries.push(PackageEntry {
            full_path,
            offset,
            length,
        });
    }

    Ok(Package {
        magic,
        header_size: cursor.position(),
        entries,
    })
}

pub fn extract_pkg(pkg_path: &Path, output_dir: &Path) -> Result<Package> {
    let metadata =
        fs::metadata(pkg_path).with_context(|| format!("Unable to stat {}", pkg_path.display()))?;
    let file_size = metadata.len();
    if file_size > MAX_PACKAGE_FILE_BYTES {
        bail!(
            "Package file size {} exceeds limit of {MAX_PACKAGE_FILE_BYTES} bytes",
            file_size
        );
    }
    let bytes =
        fs::read(pkg_path).with_context(|| format!("Unable to read {}", pkg_path.display()))?;
    let package = parse_pkg(&bytes)?;
    fs::create_dir_all(output_dir)?;
    let extraction_root = fs::canonicalize(output_dir)
        .with_context(|| format!("Unable to canonicalize {}", output_dir.display()))?;
    let header_size = usize::try_from(package.header_size)
        .context("Package header size exceeds supported platform address space")?;

    let mut total_extracted: u64 = 0;

    for entry in &package.entries {
        let relative_path = normalize_entry_path(&entry.full_path)?;
        let destination = extraction_root.join(&relative_path);
        if !destination.starts_with(&extraction_root) {
            bail!(
                "Package entry path escapes extraction root: {}",
                entry.full_path
            );
        }
        let start = header_size
            .checked_add(entry.offset as usize)
            .context("Package entry offset overflows address space")?;
        let end = start
            .checked_add(entry.length as usize)
            .context("Package entry length overflows address space")?;
        if end > bytes.len() {
            continue;
        }
        total_extracted = total_extracted
            .checked_add(entry.length as u64)
            .context("Package extraction total size overflows address space")?;
        if total_extracted > MAX_PACKAGE_TOTAL_BYTES {
            bail!(
                "Package extraction total size exceeds limit of {MAX_PACKAGE_TOTAL_BYTES} bytes"
            );
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, &bytes[start..end])?;
    }

    Ok(package)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{extract_pkg, parse_pkg};

    fn build_pkg_bytes(entry_path: &str, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(8_u32).to_le_bytes());
        bytes.extend_from_slice(b"PKGV0023");
        bytes.extend_from_slice(&(1_u32).to_le_bytes());
        bytes.extend_from_slice(&(entry_path.len() as u32).to_le_bytes());
        bytes.extend_from_slice(entry_path.as_bytes());
        bytes.extend_from_slice(&(0_u32).to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn parses_minimal_pkg_header() {
        let bytes = build_pkg_bytes("scene.json", br#"{}"#);

        let parsed = parse_pkg(&bytes).expect("package parsed");
        assert_eq!(parsed.magic, "PKGV0023");
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].full_path, "scene.json");
        assert_eq!(parsed.entries[0].offset, 0);
        assert_eq!(parsed.entries[0].length, 2);
    }

    #[test]
    fn rejects_unbounded_entry_count_before_allocation() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(8_u32).to_le_bytes());
        bytes.extend_from_slice(b"PKGV0023");
        bytes.extend_from_slice(&(u32::MAX).to_le_bytes());

        let error = parse_pkg(&bytes).expect_err("entry count should be rejected");
        assert!(
            error.to_string().contains("Package entry count"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn rejects_entry_paths_that_escape_output_root() {
        let temp = tempdir().expect("temp dir");
        let pkg_path = temp.path().join("escape.pkg");
        let output_dir = temp.path().join("extracted");
        let escaped_target = temp.path().join("escape.txt");
        fs::write(&pkg_path, build_pkg_bytes("../escape.txt", b"owned")).expect("pkg fixture");

        let error = extract_pkg(&pkg_path, &output_dir).expect_err("pkg extraction should fail");
        assert!(
            error.to_string().contains("escapes extraction root"),
            "unexpected error: {error:#}"
        );
        assert!(
            !escaped_target.exists(),
            "escape target must not be created"
        );
    }
}
