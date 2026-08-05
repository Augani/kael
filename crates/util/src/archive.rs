use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, Result};
use async_zip::base::read;
#[cfg(not(windows))]
use futures::AsyncSeek;
use futures::{AsyncRead, io::BufReader};

#[cfg(windows)]
/// Extracts a ZIP stream into `destination` after validating every entry path.
pub async fn extract_zip<R: AsyncRead + Unpin>(destination: &Path, reader: R) -> Result<()> {
    extract_zip_with_limit(destination, reader, u64::MAX).await
}

#[cfg(windows)]
/// Extracts a ZIP stream while rejecting archives whose declared expansion exceeds `max_uncompressed_bytes`.
pub async fn extract_zip_with_limit<R: AsyncRead + Unpin>(
    destination: &Path,
    reader: R,
    max_uncompressed_bytes: u64,
) -> Result<()> {
    let mut reader = read::stream::ZipFileReader::new(BufReader::new(reader));

    let destination = prepare_destination(destination)?;
    let mut uncompressed_bytes = 0;

    while let Some(mut item) = reader.next_with_entry().await? {
        let entry_reader = item.reader_mut();
        let entry = entry_reader.entry();
        account_uncompressed_bytes(
            &mut uncompressed_bytes,
            entry.uncompressed_size(),
            max_uncompressed_bytes,
        )?;
        let path = safe_entry_path(
            &destination,
            entry
                .filename()
                .as_str()
                .context("reading zip entry file name")?,
        )?;

        if entry
            .dir()
            .with_context(|| format!("reading zip entry metadata for path {path:?}"))?
        {
            std::fs::create_dir_all(&path)
                .with_context(|| format!("creating directory {path:?}"))?;
            ensure_path_is_contained(&destination, &path)?;
        } else {
            let parent_dir = path
                .parent()
                .with_context(|| format!("no parent directory for {path:?}"))?;
            std::fs::create_dir_all(parent_dir)
                .with_context(|| format!("creating parent directory {parent_dir:?}"))?;
            ensure_path_is_contained(&destination, parent_dir)?;
            reject_symlink(&path)?;
            let mut file = smol::fs::File::create(&path)
                .await
                .with_context(|| format!("creating file {path:?}"))?;
            futures::io::copy(entry_reader, &mut file)
                .await
                .with_context(|| format!("extracting into file {path:?}"))?;
        }

        reader = item.skip().await.context("reading next zip entry")?;
    }

    Ok(())
}

#[cfg(not(windows))]
/// Extracts a ZIP stream into `destination`, preserving Unix permissions.
pub async fn extract_zip<R: AsyncRead + Unpin>(destination: &Path, reader: R) -> Result<()> {
    extract_zip_with_limit(destination, reader, u64::MAX).await
}

#[cfg(not(windows))]
/// Extracts a ZIP stream while preserving permissions and bounding its declared expansion.
pub async fn extract_zip_with_limit<R: AsyncRead + Unpin>(
    destination: &Path,
    reader: R,
    max_uncompressed_bytes: u64,
) -> Result<()> {
    // Unix needs file permissions copied when extracting.
    // This is only possible to do when a reader impls `AsyncSeek` and `seek::ZipFileReader` is used.
    // `stream::ZipFileReader` also has the `unix_permissions` method, but it will always return `Some(0)`.
    //
    // A typical `reader` comes from a streaming network response, so cannot be sought right away,
    // and reading the entire archive into the memory seems wasteful.
    //
    // So, save the stream into a temporary file first and then get it read with a seeking reader.
    let mut file = async_fs::File::from(tempfile::tempfile().context("creating a temporary file")?);
    futures::io::copy(&mut BufReader::new(reader), &mut file)
        .await
        .context("saving archive contents into the temporary file")?;
    extract_seekable_zip_with_limit(destination, file, max_uncompressed_bytes).await
}

#[cfg(not(windows))]
/// Extracts a seekable ZIP reader into `destination`, preserving Unix permissions.
pub async fn extract_seekable_zip<R: AsyncRead + AsyncSeek + Unpin>(
    destination: &Path,
    reader: R,
) -> Result<()> {
    extract_seekable_zip_with_limit(destination, reader, u64::MAX).await
}

#[cfg(not(windows))]
/// Extracts a seekable ZIP reader while bounding its declared uncompressed size.
pub async fn extract_seekable_zip_with_limit<R: AsyncRead + AsyncSeek + Unpin>(
    destination: &Path,
    reader: R,
    max_uncompressed_bytes: u64,
) -> Result<()> {
    let mut reader = read::seek::ZipFileReader::new(BufReader::new(reader))
        .await
        .context("reading the zip archive")?;
    let destination = prepare_destination(destination)?;
    let mut uncompressed_bytes = 0;
    for (i, entry) in reader.file().entries().to_vec().into_iter().enumerate() {
        account_uncompressed_bytes(
            &mut uncompressed_bytes,
            entry.uncompressed_size(),
            max_uncompressed_bytes,
        )?;
        let path = safe_entry_path(
            &destination,
            entry
                .filename()
                .as_str()
                .context("reading zip entry file name")?,
        )?;

        if entry
            .dir()
            .with_context(|| format!("reading zip entry metadata for path {path:?}"))?
        {
            std::fs::create_dir_all(&path)
                .with_context(|| format!("creating directory {path:?}"))?;
            ensure_path_is_contained(&destination, &path)?;
        } else {
            let parent_dir = path
                .parent()
                .with_context(|| format!("no parent directory for {path:?}"))?;
            std::fs::create_dir_all(parent_dir)
                .with_context(|| format!("creating parent directory {parent_dir:?}"))?;
            ensure_path_is_contained(&destination, parent_dir)?;
            reject_symlink(&path)?;
            let mut file = smol::fs::File::create(&path)
                .await
                .with_context(|| format!("creating file {path:?}"))?;
            let mut entry_reader = reader
                .reader_with_entry(i)
                .await
                .with_context(|| format!("reading entry for path {path:?}"))?;
            futures::io::copy(&mut entry_reader, &mut file)
                .await
                .with_context(|| format!("extracting into file {path:?}"))?;

            if let Some(perms) = entry.unix_permissions() {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::Permissions::from_mode(u32::from(perms) & 0o777);
                file.set_permissions(permissions)
                    .await
                    .with_context(|| format!("setting permissions for file {path:?}"))?;
            }
        }
    }

    Ok(())
}

fn account_uncompressed_bytes(total: &mut u64, entry_bytes: u64, limit: u64) -> Result<()> {
    *total = total
        .checked_add(entry_bytes)
        .context("ZIP uncompressed size overflow")?;
    anyhow::ensure!(
        *total <= limit,
        "ZIP expands to more than the {limit} byte limit"
    );
    Ok(())
}

fn prepare_destination(destination: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(destination)
        .with_context(|| format!("creating archive destination {destination:?}"))?;
    destination
        .canonicalize()
        .with_context(|| format!("resolving archive destination {destination:?}"))
}

fn safe_entry_path(destination: &Path, entry_name: &str) -> Result<PathBuf> {
    anyhow::ensure!(!entry_name.is_empty(), "zip entry has an empty file name");
    let mut relative = PathBuf::new();
    for component in Path::new(entry_name).components() {
        match component {
            Component::Normal(component) => relative.push(component),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("zip entry escapes the destination: {entry_name:?}")
            }
        }
    }
    anyhow::ensure!(
        !relative.as_os_str().is_empty(),
        "zip entry has no usable file name: {entry_name:?}"
    );
    Ok(destination.join(relative))
}

fn ensure_path_is_contained(destination: &Path, path: &Path) -> Result<()> {
    let resolved = path
        .canonicalize()
        .with_context(|| format!("resolving extracted path {path:?}"))?;
    anyhow::ensure!(
        resolved.starts_with(destination),
        "zip entry resolves outside the destination: {path:?}"
    );
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!("refusing to overwrite symlink while extracting {path:?}")
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspecting extracted path {path:?}")),
    }
}

#[cfg(test)]
mod tests {
    use async_zip::ZipEntryBuilder;
    use async_zip::base::write::ZipFileWriter;
    use futures::{AsyncSeek, AsyncWriteExt};
    use smol::io::Cursor;
    use tempfile::TempDir;

    use super::*;

    async fn compress_zip(src_dir: &Path, dst: &Path) -> Result<()> {
        let mut out = smol::fs::File::create(dst).await?;
        let mut writer = ZipFileWriter::new(&mut out);

        for entry in walkdir::WalkDir::new(src_dir) {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                continue;
            }

            let relative_path = path.strip_prefix(src_dir)?;
            let data = smol::fs::read(&path).await?;

            let filename = relative_path.display().to_string();

            #[cfg(unix)]
            {
                let mut builder =
                    ZipEntryBuilder::new(filename.into(), async_zip::Compression::Deflate);
                use std::os::unix::fs::PermissionsExt;
                let metadata = std::fs::metadata(path)?;
                let perms = metadata.permissions().mode() as u16;
                builder = builder.unix_permissions(perms);
                writer.write_entry_whole(builder, &data).await?;
            }
            #[cfg(not(unix))]
            {
                let builder =
                    ZipEntryBuilder::new(filename.into(), async_zip::Compression::Deflate);
                writer.write_entry_whole(builder, &data).await?;
            }
        }

        writer.close().await?;
        out.flush().await?;

        Ok(())
    }

    #[track_caller]
    fn assert_file_content(path: &Path, content: &str) {
        assert!(path.exists(), "file not found: {:?}", path);
        let actual = std::fs::read_to_string(path).unwrap();
        assert_eq!(actual, content);
    }

    #[track_caller]
    fn make_test_data() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path();

        std::fs::write(dst.join("test"), "Hello world.").unwrap();
        std::fs::create_dir_all(dst.join("foo/bar")).unwrap();
        std::fs::write(dst.join("foo/bar.txt"), "Foo bar.").unwrap();
        std::fs::write(dst.join("foo/dar.md"), "Bar dar.").unwrap();
        std::fs::write(dst.join("foo/bar/dar你好.txt"), "你好世界").unwrap();

        dir
    }

    async fn read_archive(path: &Path) -> impl AsyncRead + AsyncSeek + Unpin {
        let data = smol::fs::read(&path).await.unwrap();
        Cursor::new(data)
    }

    #[test]
    fn test_extract_zip() {
        let test_dir = make_test_data();
        let zip_file = test_dir.path().join("test.zip");

        smol::block_on(async {
            compress_zip(test_dir.path(), &zip_file).await.unwrap();
            let reader = read_archive(&zip_file).await;

            let dir = tempfile::tempdir().unwrap();
            let dst = dir.path();
            extract_zip(dst, reader).await.unwrap();

            assert_file_content(&dst.join("test"), "Hello world.");
            assert_file_content(&dst.join("foo/bar.txt"), "Foo bar.");
            assert_file_content(&dst.join("foo/dar.md"), "Bar dar.");
            assert_file_content(&dst.join("foo/bar/dar你好.txt"), "你好世界");
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_extract_zip_preserves_executable_permissions() {
        use std::os::unix::fs::PermissionsExt;

        smol::block_on(async {
            let test_dir = tempfile::tempdir().unwrap();
            let executable_path = test_dir.path().join("my_script");

            // Create an executable file
            std::fs::write(&executable_path, "#!/bin/bash\necho 'Hello'").unwrap();
            let mut perms = std::fs::metadata(&executable_path).unwrap().permissions();
            perms.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(&executable_path, perms).unwrap();

            // Create zip
            let zip_file = test_dir.path().join("test.zip");
            compress_zip(test_dir.path(), &zip_file).await.unwrap();

            // Extract to new location
            let extract_dir = tempfile::tempdir().unwrap();
            let reader = read_archive(&zip_file).await;
            extract_zip(extract_dir.path(), reader).await.unwrap();

            // Check permissions are preserved
            let extracted_path = extract_dir.path().join("my_script");
            assert!(extracted_path.exists());
            let extracted_perms = std::fs::metadata(&extracted_path).unwrap().permissions();
            assert_eq!(extracted_perms.mode() & 0o777, 0o755);
        });
    }

    #[test]
    fn zip_entry_paths_cannot_escape_the_destination() {
        let destination = tempfile::tempdir().unwrap();
        let destination = prepare_destination(destination.path()).unwrap();

        assert!(safe_entry_path(&destination, "../outside.txt").is_err());
        assert!(safe_entry_path(&destination, "/outside.txt").is_err());
        assert_eq!(
            safe_entry_path(&destination, "nested/./file.txt").unwrap(),
            destination.join("nested/file.txt")
        );
    }

    #[test]
    fn zip_uncompressed_size_accounting_is_checked_and_bounded() {
        let mut total = 0;
        account_uncompressed_bytes(&mut total, 4, 8).unwrap();
        assert_eq!(total, 4);
        assert!(account_uncompressed_bytes(&mut total, 5, 8).is_err());

        let mut total = u64::MAX;
        assert!(account_uncompressed_bytes(&mut total, 1, u64::MAX).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn extraction_rejects_existing_symlink_targets() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = directory.path().join("outside.txt");
        let link = directory.path().join("link.txt");
        std::fs::write(&outside, "safe").unwrap();
        symlink(&outside, &link).unwrap();

        assert!(reject_symlink(&link).is_err());
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "safe");
    }
}
