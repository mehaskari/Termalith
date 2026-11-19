//! S3 and S3-compatible object storage (AWS, MinIO, Cloudflare R2,
//! Backblaze B2, Wasabi, Ceph) for the file browser (ADR-040), on
//! `object_store`. Paths look like a file system (`/logs/app.txt`); folders
//! are key prefixes ending in `/`. Nothing here logs keys or secrets.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::path::Path as Key;
use object_store::{
    ClientOptions, GetOptions, GetRange, ObjectMeta, ObjectStore, ObjectStoreExt, PutPayload,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

/// Uploads above this size use multipart upload (parts of this size).
const PART: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("access denied: check the access key, the secret key and the bucket's permissions")]
    Denied,
    #[error("{0} not found")]
    NotFound(String),
    #[error("destination already exists: {0}")]
    DestinationExists(String),
    #[error("cancelled")]
    Cancelled,
    #[error("file is larger than {limit} bytes")]
    TooLarge { limit: u64 },
    #[error("{0} was changed on the server since it was opened")]
    Modified(String),
    #[error("{0}")]
    Refused(String),
    #[error("invalid path")]
    InvalidPath,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, S3Error>;

/// Where and how to connect.
pub struct S3Config {
    /// Host name, e.g. `s3.eu-central-1.amazonaws.com`.
    pub endpoint_host: String,
    pub port: u16,
    pub https: bool,
    pub region: String,
    pub bucket: String,
    /// `https://host/bucket/key` instead of `https://bucket.host/key`
    /// (MinIO, Ceph and most self-hosted servers need it).
    pub path_style: bool,
    pub access_key: String,
    pub secret_key: Zeroizing<String>,
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Config")
            .field("endpoint_host", &self.endpoint_host)
            .field("port", &self.port)
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

/// A listing entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<u32>,
}

/// Size and modification time of an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub size: Option<u64>,
    /// Seconds since the Unix epoch.
    pub modified: Option<u32>,
}

pub struct S3Client {
    store: Arc<AmazonS3>,
}

fn map(e: object_store::Error) -> S3Error {
    use object_store::Error as E;
    match e {
        E::NotFound { path, .. } => S3Error::NotFound(path),
        E::AlreadyExists { path, .. } => S3Error::DestinationExists(path),
        E::PermissionDenied { .. } => S3Error::Denied,
        E::InvalidPath { .. } => S3Error::InvalidPath,
        other => {
            let text = other.to_string();
            if [
                "403",
                "AccessDenied",
                "SignatureDoesNotMatch",
                "InvalidAccessKeyId",
            ]
            .iter()
            .any(|m| text.contains(m))
            {
                S3Error::Denied
            } else if text.contains("error sending request") || text.contains("onnect") {
                S3Error::Connect(text)
            } else {
                S3Error::Refused(text)
            }
        }
    }
}

fn epoch(meta: &ObjectMeta) -> Option<u32> {
    u32::try_from(meta.last_modified.timestamp()).ok()
}

fn stat_of(meta: &ObjectMeta) -> Stat {
    Stat {
        size: Some(meta.size),
        modified: epoch(meta),
    }
}

/// `/a/b.txt` → key `a/b.txt`; `/` → None (the bucket root). Refuses `..`
/// and empty segments.
fn key(path: &str) -> Result<Option<Key>> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed
        .split('/')
        .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err(S3Error::InvalidPath);
    }
    Key::parse(trimmed)
        .map(Some)
        .map_err(|_| S3Error::InvalidPath)
}

fn required(path: &str) -> Result<Key> {
    key(path)?.ok_or(S3Error::InvalidPath)
}

/// A folder made here holds this empty object: S3 has no folders, and
/// `object_store` paths cannot end in `/` (the usual `name/` marker).
/// Listings hide it.
const KEEP: &str = ".keep";

fn keep_object(path: &str) -> Result<Key> {
    Ok(required(path)?.join(KEEP))
}

impl S3Client {
    /// Builds the client and checks the bucket can be listed.
    pub async fn connect(config: &S3Config) -> Result<Self> {
        let scheme = if config.https { "https" } else { "http" };
        let default = if config.https { 443 } else { 80 };
        let host = config.endpoint_host.trim();
        if host.is_empty() || host.contains('/') || host.chars().any(char::is_whitespace) {
            return Err(S3Error::Connect("invalid endpoint host".into()));
        }
        let endpoint = if config.port == default {
            format!("{scheme}://{host}")
        } else {
            format!("{scheme}://{host}:{}", config.port)
        };
        let region = match config.region.trim() {
            "" => "us-east-1",
            r => r,
        };
        let store = AmazonS3Builder::new()
            .with_endpoint(endpoint)
            .with_region(region)
            .with_bucket_name(config.bucket.trim())
            .with_access_key_id(config.access_key.trim())
            .with_secret_access_key(config.secret_key.as_str())
            .with_virtual_hosted_style_request(!config.path_style)
            // Fail fast on a wrong address instead of ten retries.
            .with_retry(object_store::RetryConfig {
                max_retries: 2,
                retry_timeout: Duration::from_secs(20),
                ..object_store::RetryConfig::default()
            })
            // Set in the client options: `with_client_options` replaces them.
            .with_client_options(
                ClientOptions::new()
                    .with_allow_http(!config.https)
                    .with_timeout(TIMEOUT)
                    .with_connect_timeout(Duration::from_secs(15)),
            )
            .build()
            .map_err(|e| S3Error::Connect(e.to_string()))?;
        let client = Self {
            store: Arc::new(store),
        };
        client.list("/").await?;
        Ok(client)
    }

    pub async fn home(&self) -> Result<String> {
        Ok("/".into())
    }

    /// Folders (common prefixes) and objects directly under `dir`.
    pub async fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let prefix = key(dir)?;
        let listed = self
            .store
            .list_with_delimiter(prefix.as_ref())
            .await
            .map_err(map)?;
        let mut out = Vec::new();
        for p in listed.common_prefixes {
            if let Some(name) = p.filename() {
                out.push(Entry {
                    name: name.to_owned(),
                    dir: true,
                    size: 0,
                    modified: None,
                });
            }
        }
        for o in listed.objects {
            // A folder's own marker (made by other tools) and our .keep.
            if prefix.as_ref() == Some(&o.location) {
                continue;
            }
            match o.location.filename() {
                Some(name) if !name.is_empty() && name != KEEP => out.push(Entry {
                    name: name.to_owned(),
                    dir: false,
                    size: o.size,
                    modified: epoch(&o),
                }),
                // The marker of this folder itself.
                _ => {}
            }
        }
        Ok(out)
    }

    pub async fn stat(&self, remote: &str) -> Result<Stat> {
        let meta = self.store.head(&required(remote)?).await.map_err(map)?;
        Ok(stat_of(&meta))
    }

    async fn exists(&self, k: &Key) -> Result<bool> {
        match self.store.head(k).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(map(e)),
        }
    }

    /// Uploads a local file; multipart above 8 MiB, aborted on failure.
    pub async fn upload(
        &self,
        local: &Path,
        remote: &str,
        overwrite: bool,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64> {
        let k = required(remote)?;
        if !overwrite && self.exists(&k).await? {
            return Err(S3Error::DestinationExists(remote.to_owned()));
        }
        let mut file = tokio::fs::File::open(local).await?;
        let size = file.metadata().await?.len();
        if size <= PART as u64 {
            let mut data = Vec::with_capacity(size as usize);
            file.read_to_end(&mut data).await?;
            if cancel.is_cancelled() {
                return Err(S3Error::Cancelled);
            }
            self.store
                .put(&k, PutPayload::from(data))
                .await
                .map_err(map)?;
            progress(size);
            return Ok(size);
        }
        let mut upload = self.store.put_multipart(&k).await.map_err(map)?;
        let mut sent: u64 = 0;
        let result: Result<()> = async {
            loop {
                let mut part = Vec::with_capacity(PART);
                (&mut file).take(PART as u64).read_to_end(&mut part).await?;
                if part.is_empty() {
                    break;
                }
                if cancel.is_cancelled() {
                    return Err(S3Error::Cancelled);
                }
                let n = part.len() as u64;
                upload
                    .put_part(PutPayload::from(Bytes::from(part)))
                    .await
                    .map_err(map)?;
                sent += n;
                progress(sent);
            }
            upload.complete().await.map_err(map)?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = upload.abort().await;
        }
        result.map(|()| sent)
    }

    async fn write_stream(
        &self,
        k: &Key,
        range: Option<GetRange>,
        dst: &mut tokio::fs::File,
        cancel: &CancellationToken,
        progress: &mut (impl FnMut(u64) + Send),
    ) -> Result<u64> {
        let got = self
            .store
            .get_opts(
                k,
                GetOptions {
                    range,
                    ..GetOptions::default()
                },
            )
            .await
            .map_err(map)?;
        let mut stream = got.into_stream();
        let mut n: u64 = 0;
        loop {
            let chunk = tokio::select! {
                () = cancel.cancelled() => return Err(S3Error::Cancelled),
                c = stream.next() => c,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(map)?;
            dst.write_all(&chunk).await?;
            n += chunk.len() as u64;
            progress(n);
        }
        dst.flush().await?;
        Ok(n)
    }

    /// Downloads to `local` through a temporary file next to it.
    pub async fn download(
        &self,
        remote: &str,
        local: &Path,
        overwrite: bool,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64> {
        let k = required(remote)?;
        if !overwrite && tokio::fs::try_exists(local).await.unwrap_or(false) {
            return Err(S3Error::DestinationExists(local.display().to_string()));
        }
        let mut name = local.file_name().unwrap_or_default().to_os_string();
        name.push(".termalith-tmp");
        let tmp = local.with_file_name(name);
        let mut dst = tokio::fs::File::create(&tmp).await?;
        let result = self
            .write_stream(&k, None, &mut dst, cancel, &mut progress)
            .await;
        drop(dst);
        match result {
            Ok(n) => {
                tokio::fs::rename(&tmp, local).await?;
                Ok(n)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(e)
            }
        }
    }

    /// Downloads through `partial`, continuing after the bytes it holds
    /// (a ranged GET); it becomes `local` when complete. Returns the full
    /// length.
    pub async fn download_resume(
        &self,
        remote: &str,
        local: &Path,
        partial: &Path,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64> {
        let k = required(remote)?;
        let size = self.store.head(&k).await.map_err(map)?.size;
        let mut offset = tokio::fs::metadata(partial)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if offset > size {
            tokio::fs::remove_file(partial).await?;
            offset = 0;
        }
        let mut dst = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(partial)
            .await?;
        if offset < size {
            let range = (offset > 0).then_some(GetRange::Offset(offset));
            let mut report = |n: u64| progress(offset + n);
            self.write_stream(&k, range, &mut dst, cancel, &mut report)
                .await?;
        }
        dst.sync_all().await?;
        drop(dst);
        tokio::fs::rename(partial, local).await?;
        Ok(size)
    }

    pub async fn read_file(&self, remote: &str, limit: u64) -> Result<(Vec<u8>, Stat)> {
        let k = required(remote)?;
        let meta = self.store.head(&k).await.map_err(map)?;
        if meta.size > limit {
            return Err(S3Error::TooLarge { limit });
        }
        let data = self
            .store
            .get(&k)
            .await
            .map_err(map)?
            .bytes()
            .await
            .map_err(map)?;
        Ok((data.to_vec(), stat_of(&meta)))
    }

    /// Saves `data` unless the object changed since `expected` was read.
    pub async fn replace_file(&self, remote: &str, data: &[u8], expected: &Stat) -> Result<Stat> {
        let k = required(remote)?;
        if self.stat(remote).await? != *expected {
            return Err(S3Error::Modified(remote.to_owned()));
        }
        self.store
            .put(&k, PutPayload::from(Bytes::copy_from_slice(data)))
            .await
            .map_err(map)?;
        self.stat(remote).await
    }

    pub async fn create_dir(&self, remote: &str) -> Result<()> {
        let keep = keep_object(remote)?;
        if self.exists(&required(remote)?).await?
            || self.exists(&keep).await?
            || !self.list(remote).await?.is_empty()
        {
            return Err(S3Error::DestinationExists(remote.to_owned()));
        }
        self.store
            .put(&keep, PutPayload::default())
            .await
            .map_err(map)?;
        Ok(())
    }

    /// Renames a file (copy, then delete). Folders cannot be renamed.
    pub async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (src, dst) = (required(from)?, required(to)?);
        if !self.exists(&src).await? {
            return Err(S3Error::Refused(
                "only files can be renamed on S3 (folders are key prefixes)".into(),
            ));
        }
        if self.exists(&dst).await? {
            return Err(S3Error::DestinationExists(to.to_owned()));
        }
        self.store.copy(&src, &dst).await.map_err(map)?;
        self.store.delete(&src).await.map_err(map)
    }

    /// Deletes a file, or an empty folder (its `.keep`).
    pub async fn delete(&self, remote: &str) -> Result<()> {
        let k = required(remote)?;
        if self.exists(&k).await? {
            return self.store.delete(&k).await.map_err(map);
        }
        if !self.list(remote).await?.is_empty() {
            return Err(S3Error::Refused("the folder is not empty".into()));
        }
        let keep = keep_object(remote)?;
        if self.exists(&keep).await? {
            return self.store.delete(&keep).await.map_err(map);
        }
        Err(S3Error::NotFound(remote.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_map_to_keys_and_refuse_escapes() {
        assert!(key("/").unwrap().is_none());
        assert_eq!(
            key("/logs/app.txt").unwrap().unwrap().as_ref(),
            "logs/app.txt"
        );
        assert_eq!(key("logs/").unwrap().unwrap().as_ref(), "logs");
        assert!(key("/a/../b").is_err());
        assert!(key("/a//b").is_err());
    }
}
