//! Object storage: S3-compatible store construction and tiny put/get helpers.

use std::sync::Arc;

use object_store::aws::AmazonS3Builder;
use object_store::{path::Path, ObjectStore, ObjectStoreExt, PutPayload};

use crate::backup::{BackupError, Config};

/// Install the ring-based rustls crypto provider (idempotent). reqwest is
/// built with `rustls-no-provider` to keep the dependency tree free of
/// C builds; this must run before any TLS handshake, i.e. before any S3
/// request. Also called by test harnesses that build reqwest clients.
pub fn init_rustls_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Build the S3-compatible store for the given configuration.
///
/// When `endpoint` is set, requests use path-style URLs (the default), which
/// is what Backblaze B2 and MinIO expect. Credentials are static (application
/// key) credentials. Building performs no network I/O.
pub fn build_store(config: &Config) -> Result<Arc<dyn ObjectStore>, BackupError> {
    init_rustls_crypto();
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&config.bucket)
        .with_region(&config.region)
        .with_access_key_id(&config.access_key)
        .with_secret_access_key(&config.secret_key);
    if let Some(endpoint) = &config.endpoint {
        if endpoint.starts_with("http://") {
            // object_store only allows HTTP for explicitly configured endpoints.
            builder = builder.with_allow_http(true);
        }
        builder = builder.with_endpoint(endpoint);
    }
    Ok(Arc::new(builder.build()?))
}

pub(crate) async fn put(
    store: &dyn ObjectStore,
    key: &str,
    payload: Vec<u8>,
) -> Result<(), BackupError> {
    store
        .put(&Path::from(key), PutPayload::from(payload))
        .await?;
    Ok(())
}

pub(crate) async fn get(store: &dyn ObjectStore, key: &str) -> Result<Vec<u8>, BackupError> {
    let result = store.get(&Path::from(key)).await?;
    let bytes = result.bytes().await?;
    Ok(bytes.to_vec())
}
