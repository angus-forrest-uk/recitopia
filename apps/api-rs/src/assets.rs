use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{config::AssetConfig, runtime::generate_id};

pub const MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 4_096;
pub const MAX_ARCHIVE_PATH_BYTES: usize = 1_024;
pub const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_PAGE_IMAGE_UPLOAD_BODY_BYTES: usize = 80 * 1024 * 1024;
pub const IMAGE_DERIVATIVE_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const IMAGE_DERIVATIVE_MAX_DIMENSION: u32 = 1_600;
pub const IMAGE_DERIVATIVE_QUALITY: u8 = 45;

#[derive(Clone, Debug)]
pub struct AssetManager {
    config: AssetConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredImage {
    pub image_path: String,
    pub image_hash: String,
    pub size_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveImage {
    pub image_index: u32,
    pub image_path: String,
    pub image_hash: String,
}

impl AssetManager {
    #[must_use]
    pub fn new(config: AssetConfig) -> Self {
        Self { config }
    }

    /// Creates the private work directory and returns the path used to stream
    /// one browser-generated tar upload before it is inspected.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError`] when the directory cannot be created.
    pub fn archive_upload_path(&self, import_id: &str) -> Result<PathBuf, AssetError> {
        let directory = self
            .config
            .import_dir
            .join("cookbook-archives")
            .join(import_id);
        fs::create_dir_all(&directory).map_err(|source| AssetError::Io {
            operation: "create archive directory",
            source,
        })?;
        Ok(directory.join("upload.tar"))
    }

    /// Decodes a JSON data-URI/base64 image and writes it under its SHA-256.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError`] for malformed base64, oversized images, or I/O failures.
    pub fn store_base64_image(
        &self,
        encoded: &str,
        mime_type: &str,
    ) -> Result<StoredImage, AssetError> {
        let source = encoded
            .find("base64,")
            .map_or(encoded, |index| &encoded[index + "base64,".len()..])
            .trim();
        let bytes = STANDARD
            .decode(source)
            .map_err(|_| AssetError::InvalidBase64)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_IMAGE_BYTES {
            return Err(AssetError::ImageTooLarge);
        }
        self.store_image_bytes(&bytes, extension_from_mime(mime_type))
    }

    /// Inspects a tar from disk, extracts only regular supported images, and
    /// stores each image by SHA-256. Archive order becomes image order.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError`] for malformed/unsafe archives, duplicate images,
    /// limits, or I/O failures.
    pub fn ingest_archive(
        &self,
        archive_path: &Path,
        import_id: &str,
    ) -> Result<Vec<ArchiveImage>, AssetError> {
        let archive = fs::File::open(archive_path).map_err(|source| AssetError::Io {
            operation: "open archive",
            source,
        })?;
        let extracted_dir = self
            .config
            .import_dir
            .join("cookbook-archives")
            .join(import_id)
            .join("extracted");
        fs::create_dir_all(&extracted_dir).map_err(|source| AssetError::Io {
            operation: "create archive extraction directory",
            source,
        })?;

        let mut images = Vec::new();
        let mut hashes = HashSet::new();
        walk_archive(archive, |entry_path, bytes| {
            let hash = sha256_hex(bytes);
            if !hashes.insert(hash.clone()) {
                return Err(AssetError::DuplicateImage);
            }
            let extension = extension_from_file_name(entry_path);
            let extracted_path = extracted_dir.join(entry_path);
            if let Some(parent) = extracted_path.parent() {
                fs::create_dir_all(parent).map_err(|source| AssetError::Io {
                    operation: "create extracted image directory",
                    source,
                })?;
            }
            atomic_write(&extracted_path, bytes)?;
            let stored = self.store_image_bytes_with_hash(bytes, extension, &hash)?;
            let image_index =
                u32::try_from(images.len() + 1).map_err(|_| AssetError::TooManyEntries)?;
            images.push(ArchiveImage {
                image_index,
                image_path: stored.image_path,
                image_hash: stored.image_hash,
            });
            Ok(())
        })?;
        Ok(images)
    }

    /// Returns a cached AVIF derivative, encoding it atomically on first use.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError`] when no converter is configured or conversion fails.
    pub fn avif_derivative(
        &self,
        source_path: &Path,
        image_hash: &str,
    ) -> Result<PathBuf, AssetError> {
        if !is_sha256_hex(image_hash) {
            return Err(AssetError::InvalidImageHash);
        }
        let convert_bin = self
            .config
            .image_convert_bin
            .as_ref()
            .ok_or(AssetError::ConverterUnavailable)?;
        let directory = self.config.import_dir.join("derived");
        fs::create_dir_all(&directory).map_err(|source| AssetError::Io {
            operation: "create derivative directory",
            source,
        })?;
        let target = directory.join(format!(
            "{image_hash}-w{IMAGE_DERIVATIVE_MAX_DIMENSION}.avif"
        ));
        if is_bounded_file(&target, IMAGE_DERIVATIVE_MAX_BYTES) {
            return Ok(target);
        }

        let temporary = directory.join(format!(".{image_hash}-{}.avif", generate_id("derivative")));
        let status = Command::new(convert_bin)
            .arg(source_path)
            .arg("-resize")
            .arg(format!(
                "{IMAGE_DERIVATIVE_MAX_DIMENSION}x{IMAGE_DERIVATIVE_MAX_DIMENSION}>"
            ))
            .arg("-quality")
            .arg(IMAGE_DERIVATIVE_QUALITY.to_string())
            .arg(&temporary)
            .status()
            .map_err(|source| AssetError::Io {
                operation: "run image converter",
                source,
            })?;
        if !status.success() || !is_bounded_file(&temporary, IMAGE_DERIVATIVE_MAX_BYTES) {
            let _ = fs::remove_file(&temporary);
            return Err(AssetError::ImageEncodeFailed);
        }
        if let Err(source) = fs::rename(&temporary, &target) {
            if !is_bounded_file(&target, IMAGE_DERIVATIVE_MAX_BYTES) {
                let _ = fs::remove_file(&temporary);
                return Err(AssetError::Io {
                    operation: "publish image derivative",
                    source,
                });
            }
            let _ = fs::remove_file(&temporary);
        }
        Ok(target)
    }

    fn store_image_bytes(
        &self,
        bytes: &[u8],
        extension: &'static str,
    ) -> Result<StoredImage, AssetError> {
        let hash = sha256_hex(bytes);
        self.store_image_bytes_with_hash(bytes, extension, &hash)
    }

    fn store_image_bytes_with_hash(
        &self,
        bytes: &[u8],
        extension: &'static str,
        hash: &str,
    ) -> Result<StoredImage, AssetError> {
        let directory = self.config.import_dir.join("cookbook-images");
        fs::create_dir_all(&directory).map_err(|source| AssetError::Io {
            operation: "create cookbook image directory",
            source,
        })?;
        let path = directory.join(format!("{hash}.{extension}"));
        atomic_write(&path, bytes)?;
        Ok(StoredImage {
            image_path: path.to_string_lossy().into_owned(),
            image_hash: hash.to_owned(),
            size_bytes: bytes.len(),
        })
    }
}

/// Exercises the production tar parser without writing files.
///
/// # Errors
///
/// Returns [`AssetError`] for malformed, unsafe, empty, or oversized archives.
pub fn validate_archive_bytes(bytes: &[u8]) -> Result<usize, AssetError> {
    walk_archive(Cursor::new(bytes), |_path, _bytes| Ok(()))
}

fn walk_archive<R, F>(reader: R, mut visit: F) -> Result<usize, AssetError>
where
    R: Read,
    F: FnMut(&str, &[u8]) -> Result<(), AssetError>,
{
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().map_err(|_| AssetError::InvalidArchive)?;
    let mut entry_count = 0_usize;
    let mut image_count = 0_usize;
    let mut image_bytes = 0_u64;

    for entry in entries {
        entry_count = entry_count
            .checked_add(1)
            .ok_or(AssetError::TooManyEntries)?;
        if entry_count > MAX_ARCHIVE_ENTRIES {
            return Err(AssetError::TooManyEntries);
        }
        let mut entry = entry.map_err(|_| AssetError::InvalidArchive)?;
        let path_bytes = entry.path_bytes();
        if path_bytes.len() > MAX_ARCHIVE_PATH_BYTES {
            return Err(AssetError::UnsafeArchivePath);
        }
        let path = std::str::from_utf8(&path_bytes)
            .map_err(|_| AssetError::UnsafeArchivePath)?
            .to_owned();
        if !is_safe_archive_path(&path) {
            return Err(AssetError::UnsafeArchivePath);
        }

        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
            return Err(AssetError::UnsupportedArchiveEntry);
        }
        if !is_supported_image_file_name(&path) {
            continue;
        }
        let declared_size = entry.size();
        if declared_size > MAX_IMAGE_BYTES {
            return Err(AssetError::ImageTooLarge);
        }
        image_bytes = image_bytes
            .checked_add(declared_size)
            .ok_or(AssetError::ArchiveTooLarge)?;
        if image_bytes > MAX_ARCHIVE_BYTES {
            return Err(AssetError::ArchiveTooLarge);
        }
        let capacity = usize::try_from(declared_size).map_err(|_| AssetError::ImageTooLarge)?;
        let mut bytes = Vec::with_capacity(capacity);
        Read::take(&mut entry, MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| AssetError::InvalidArchive)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != declared_size {
            return Err(AssetError::InvalidArchive);
        }
        visit(&path, &bytes)?;
        image_count += 1;
    }

    if image_count == 0 {
        return Err(AssetError::EmptyArchive);
    }
    Ok(image_count)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    if path.is_file() {
        return Ok(());
    }
    let parent = path.parent().ok_or_else(|| AssetError::Io {
        operation: "resolve asset parent",
        source: io::Error::new(io::ErrorKind::InvalidInput, "asset has no parent"),
    })?;
    let temporary = parent.join(format!(".{}.tmp", generate_id("asset")));
    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    match write_result {
        Ok(()) => Ok(()),
        Err(_) if path.is_file() => {
            let _ = fs::remove_file(&temporary);
            Ok(())
        }
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            Err(AssetError::Io {
                operation: "write content-addressed asset",
                source,
            })
        }
    }
}

fn is_bounded_file(path: &Path, limit: u64) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= limit)
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

#[must_use]
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn extension_from_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" => "heic",
        _ => "jpg",
    }
}

fn extension_from_file_name(file_name: &str) -> &'static str {
    match Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "png",
        Some("webp") => "webp",
        Some("heic") => "heic",
        _ => "jpg",
    }
}

fn is_supported_image_file_name(file_name: &str) -> bool {
    matches!(
        Path::new(file_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "heic")
    )
}

fn is_safe_archive_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("invalid base64 image")]
    InvalidBase64,
    #[error("archive is malformed")]
    InvalidArchive,
    #[error("archive contains no supported images")]
    EmptyArchive,
    #[error("archive contains an unsafe path")]
    UnsafeArchivePath,
    #[error("archive contains a link or unsupported entry type")]
    UnsupportedArchiveEntry,
    #[error("archive contains too many entries")]
    TooManyEntries,
    #[error("archive is too large")]
    ArchiveTooLarge,
    #[error("image is too large")]
    ImageTooLarge,
    #[error("archive contains duplicate image content")]
    DuplicateImage,
    #[error("invalid image hash")]
    InvalidImageHash,
    #[error("image converter is not configured")]
    ConverterUnavailable,
    #[error("image conversion failed")]
    ImageEncodeFailed,
    #[error("asset I/O failed during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(directory: &Path) -> AssetManager {
        AssetManager::new(AssetConfig {
            import_dir: directory.to_owned(),
            image_convert_bin: None,
        })
    }

    fn tar_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut bytes);
            for (path, contents) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(u64::try_from(contents.len()).unwrap());
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, *contents)
                    .expect("append tar file");
            }
            builder.finish().expect("finish tar");
        }
        bytes
    }

    #[test]
    fn hashes_and_decodes_data_uri_images() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let directory = tempfile::tempdir().unwrap();
        let stored = manager(directory.path())
            .store_base64_image("data:image/png;base64,aGVsbG8=", "image/png")
            .expect("store image");
        assert_eq!(stored.size_bytes, 5);
        assert!(
            Path::new(&stored.image_path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        );
        assert_eq!(fs::read(stored.image_path).unwrap(), b"hello");
    }

    #[test]
    fn validates_and_ingests_regular_images_in_archive_order() {
        let directory = tempfile::tempdir().unwrap();
        let archive_bytes = tar_bytes(&[("001.jpg", b"first"), ("nested/002.png", b"second")]);
        assert_eq!(validate_archive_bytes(&archive_bytes).unwrap(), 2);
        let archive_path = directory.path().join("upload.tar");
        fs::write(&archive_path, archive_bytes).unwrap();
        let images = manager(directory.path())
            .ingest_archive(&archive_path, "cookbook-import-test")
            .expect("ingest archive");
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].image_index, 1);
        assert!(
            Path::new(&images[1].image_path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        );
    }

    #[test]
    fn rejects_empty_and_duplicate_image_archives() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            validate_archive_bytes(&tar_bytes(&[("notes.txt", b"not an image")])),
            Err(AssetError::EmptyArchive)
        ));
        let archive_path = directory.path().join("duplicates.tar");
        fs::write(
            &archive_path,
            tar_bytes(&[("001.jpg", b"same"), ("002.jpg", b"same")]),
        )
        .unwrap();
        assert!(matches!(
            manager(directory.path()).ingest_archive(&archive_path, "duplicate-import"),
            Err(AssetError::DuplicateImage)
        ));
    }

    #[test]
    fn rejects_links_instead_of_extracting_them() {
        let mut bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut bytes);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name("../../outside").unwrap();
            header.set_cksum();
            builder
                .append_data(&mut header, "linked.jpg", io::empty())
                .unwrap();
            builder.finish().unwrap();
        }
        assert!(matches!(
            validate_archive_bytes(&bytes),
            Err(AssetError::UnsupportedArchiveEntry)
        ));
    }
}
