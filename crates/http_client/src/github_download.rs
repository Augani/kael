use std::{path::Path, pin::Pin, task::Poll};

use anyhow::{Context, Result, bail};
use async_compression::futures::bufread::GzipDecoder;
use futures::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite, io::BufReader};
use http::header::CONTENT_LENGTH;
use sha2::{Digest, Sha256};

use crate::{HttpClient, github::AssetKind};

const MAX_COMPRESSED_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTRACTED_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn parse_sha_256(digest: &str) -> Result<String> {
    let digest = digest.strip_prefix("sha256:").unwrap_or(digest);
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("expected SHA-256 must contain exactly 64 hexadecimal characters");
    }
    Ok(digest.to_ascii_lowercase())
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct GithubBinaryMetadata {
    pub metadata_version: u64,
    pub digest: Option<String>,
}

impl GithubBinaryMetadata {
    pub async fn read_from_file(metadata_path: &Path) -> Result<GithubBinaryMetadata> {
        let metadata_content = async_fs::read_to_string(metadata_path)
            .await
            .with_context(|| format!("reading metadata file at {metadata_path:?}"))?;
        serde_json::from_str(&metadata_content)
            .with_context(|| format!("parsing metadata file at {metadata_path:?}"))
    }

    pub async fn write_to_file(&self, metadata_path: &Path) -> Result<()> {
        let metadata_content = serde_json::to_string(self)
            .with_context(|| format!("serializing metadata for {metadata_path:?}"))?;
        async_fs::write(metadata_path, metadata_content.as_bytes())
            .await
            .with_context(|| format!("writing metadata file at {metadata_path:?}"))?;
        Ok(())
    }
}

pub async fn download_server_binary(
    http_client: &dyn HttpClient,
    url: &str,
    digest: Option<&str>,
    destination_path: &Path,
    asset_kind: AssetKind,
) -> Result<(), anyhow::Error> {
    let expected_sha_256 = digest.map(parse_sha_256).transpose()?;
    log::info!("downloading github artifact from {url}");
    let mut response = http_client
        .get(url, Default::default(), true)
        .await
        .with_context(|| format!("downloading release from {url}"))?;
    if !response.status().is_success() {
        bail!(
            "downloading {url} failed with HTTP status {}",
            response.status()
        );
    }

    if let Some(content_length) = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        anyhow::ensure!(
            content_length <= MAX_COMPRESSED_ARCHIVE_BYTES,
            "{url} archive is {content_length} bytes, exceeding the {MAX_COMPRESSED_ARCHIVE_BYTES} byte limit"
        );
    }

    // Stage every response before extraction. Besides making ZIP seekable, this
    // avoids leaving an extraction blocked on a network stream and gives all
    // archive formats the same size and digest checks.
    let temp_asset_file = tempfile::NamedTempFile::new()
        .with_context(|| format!("creating a temporary file for {url}"))?;
    let (temp_asset_file, _temp_guard) = temp_asset_file.into_parts();
    let mut writer = HashingWriter {
        writer: async_fs::File::from(temp_asset_file),
        hasher: Sha256::new(),
    };
    let mut limited_body = response
        .body_mut()
        .take(MAX_COMPRESSED_ARCHIVE_BYTES.saturating_add(1));
    let copied = futures::io::copy(&mut limited_body, &mut writer)
        .await
        .with_context(|| format!("saving archive contents into a temporary file for {url}"))?;
    anyhow::ensure!(
        copied <= MAX_COMPRESSED_ARCHIVE_BYTES,
        "{url} archive exceeds the {MAX_COMPRESSED_ARCHIVE_BYTES} byte limit"
    );

    let asset_sha_256 = format!("{:x}", writer.hasher.finalize_reset());
    if let Some(expected_sha_256) = expected_sha_256 {
        anyhow::ensure!(
            asset_sha_256 == expected_sha_256,
            "{url} asset got SHA-256 mismatch. Expected: {expected_sha_256}, Got: {asset_sha_256}",
        );
    }

    writer
        .writer
        .seek(std::io::SeekFrom::Start(0))
        .await
        .with_context(|| format!("seeking temporary file for {destination_path:?}"))?;
    stream_file_archive(&mut writer.writer, url, destination_path, asset_kind)
        .await
        .with_context(|| {
            format!("extracting downloaded asset for {url} into {destination_path:?}")
        })?;
    Ok(())
}

async fn stream_file_archive(
    file_archive: impl AsyncRead + AsyncSeek + Unpin,
    url: &str,
    destination_path: &Path,
    asset_kind: AssetKind,
) -> Result<()> {
    match asset_kind {
        AssetKind::TarGz => extract_tar_gz(destination_path, url, file_archive).await?,
        AssetKind::Gz => extract_gz(destination_path, url, file_archive).await?,
        #[cfg(not(windows))]
        AssetKind::Zip => {
            util::archive::extract_seekable_zip_with_limit(
                destination_path,
                file_archive,
                MAX_EXTRACTED_ARCHIVE_BYTES,
            )
            .await?;
        }
        #[cfg(windows)]
        AssetKind::Zip => {
            util::archive::extract_zip_with_limit(
                destination_path,
                file_archive,
                MAX_EXTRACTED_ARCHIVE_BYTES,
            )
            .await?;
        }
    };
    Ok(())
}

async fn extract_tar_gz(
    destination_path: &Path,
    url: &str,
    from: impl AsyncRead + Unpin,
) -> Result<(), anyhow::Error> {
    let decompressed_bytes =
        GzipDecoder::new(BufReader::new(from)).take(MAX_EXTRACTED_ARCHIVE_BYTES.saturating_add(1));
    let mut archive = async_tar::Archive::new(decompressed_bytes);
    archive
        .unpack(&destination_path)
        .await
        .with_context(|| format!("extracting {url} to {destination_path:?}"))?;
    Ok(())
}

async fn extract_gz(
    destination_path: &Path,
    url: &str,
    from: impl AsyncRead + Unpin,
) -> Result<(), anyhow::Error> {
    let mut decompressed_bytes = GzipDecoder::new(BufReader::new(from));
    let mut file = async_fs::File::create(&destination_path)
        .await
        .with_context(|| {
            format!("creating a file {destination_path:?} for a download from {url}")
        })?;
    let copied = futures::io::copy(&mut decompressed_bytes, &mut file)
        .await
        .with_context(|| format!("extracting {url} to {destination_path:?}"))?;
    anyhow::ensure!(
        copied <= MAX_EXTRACTED_ARCHIVE_BYTES,
        "{url} expands beyond the {MAX_EXTRACTED_ARCHIVE_BYTES} byte limit"
    );
    Ok(())
}

struct HashingWriter<W: AsyncWrite + Unpin> {
    writer: W,
    hasher: Sha256,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for HashingWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        match Pin::new(&mut self.writer).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                self.hasher.update(&buf[..n]);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        Pin::new(&mut self.writer).poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures::{FutureExt as _, future::BoxFuture};
    use http::{HeaderValue, Request, Response, StatusCode};

    use super::*;
    use crate::AsyncBody;

    struct StaticClient {
        status: StatusCode,
        body: Arc<[u8]>,
    }

    impl HttpClient for StaticClient {
        fn type_name(&self) -> &'static str {
            "StaticClient"
        }

        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn send(
            &self,
            _request: Request<AsyncBody>,
        ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
            let status = self.status;
            let body = self.body.to_vec();
            async move {
                Ok(Response::builder()
                    .status(status)
                    .body(AsyncBody::from(body))?)
            }
            .boxed()
        }

        fn proxy(&self) -> Option<&url::Url> {
            None
        }
    }

    #[test]
    fn validates_and_normalizes_sha_256_values() {
        let uppercase = "A".repeat(64);
        assert_eq!(parse_sha_256(&uppercase).unwrap(), "a".repeat(64));
        assert_eq!(
            parse_sha_256(&format!("sha256:{uppercase}")).unwrap(),
            "a".repeat(64)
        );
        assert!(parse_sha_256("abc").is_err());
        assert!(parse_sha_256(&"g".repeat(64)).is_err());
    }

    #[test]
    fn rejects_http_errors_before_creating_a_destination() {
        let client = StaticClient {
            status: StatusCode::NOT_FOUND,
            body: Arc::from(&b"not found"[..]),
        };
        let destination = tempfile::tempdir().unwrap().path().join("asset");

        let error = futures::executor::block_on(download_server_binary(
            &client,
            "https://example.test/missing.gz",
            None,
            &destination,
            AssetKind::Gz,
        ))
        .unwrap_err();

        assert!(error.to_string().contains("404"));
        assert!(!destination.exists());
    }

    #[test]
    fn verifies_and_extracts_a_staged_gzip_download() {
        futures::executor::block_on(async {
            let source = b"verified artifact";
            let mut encoder = async_compression::futures::bufread::GzipEncoder::new(
                BufReader::new(futures::io::Cursor::new(source)),
            );
            let mut compressed = Vec::new();
            encoder.read_to_end(&mut compressed).await.unwrap();
            let digest = format!("sha256:{:x}", Sha256::digest(&compressed));
            let client = StaticClient {
                status: StatusCode::OK,
                body: Arc::from(compressed),
            };
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("asset");

            download_server_binary(
                &client,
                "https://example.test/asset.gz",
                Some(&digest),
                &destination,
                AssetKind::Gz,
            )
            .await
            .unwrap();

            assert_eq!(async_fs::read(destination).await.unwrap(), source);
        });
    }
}
