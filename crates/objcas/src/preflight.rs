// SPDX-License-Identifier: MPL-2.0
//! Backend capability checks that need no application types: the
//! `preflight` run before a store is first used, and the conditional-write
//! probe (see also [`crate::probe`] for the deeper CAS semantics probe).

use crate::error::Error;
use futures::StreamExt;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion};
use uuid::Uuid;

/// Exercise every object-store operation an application
/// relies on against a throwaway key under the configured prefix, so
/// a create command can refuse a backend up front with one clean message
/// instead of failing mid-create with a raw protocol error (e.g.
/// Backblaze B2, which 501s the `If-None-Match`/`If-Match` headers
/// that create-if-absent and etag CAS depend on).
///
/// Every check runs even when an earlier one fails, so the report
/// lists all problems at once. Cleanup is best-effort.
pub async fn preflight(store: &dyn ObjectStore) -> Vec<PreflightCheck> {
    let key = object_store::path::Path::from(format!(".preflight/{}", Uuid::new_v4()));
    let mut checks = Vec::new();

    // Unconditional write: connectivity, credentials, bucket/prefix
    // write permission. Its version feeds the etag-CAS check below.
    let write = store
        .put_opts(
            &key,
            PutPayload::from_static(b"preflight"),
            PutOptions::from(PutMode::Overwrite),
        )
        .await;
    let written = match &write {
        Ok(r) => Some(UpdateVersion {
            e_tag: r.e_tag.clone(),
            version: r.version.clone(),
        }),
        Err(_) => None,
    };
    checks.push(PreflightCheck::new(
        "write object (PUT)",
        true,
        write.map(|_| ()).map_err(|e| concise_os_error(&e)),
    ));

    // Read back.
    let read = match store.get(&key).await {
        Ok(r) => r.bytes().await.map(|_| ()),
        Err(e) => Err(e),
    };
    checks.push(PreflightCheck::new(
        "read object (GET)",
        true,
        read.map_err(|e| concise_os_error(&e)),
    ));

    // List the prefix (GC and node discovery walk object listings).
    let list = store
        .list(Some(&object_store::path::Path::from(".preflight")))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>();
    checks.push(PreflightCheck::new(
        "list objects (LIST)",
        true,
        list.map(|_| ()).map_err(|e| concise_os_error(&e)),
    ));

    // Create-if-absent (`If-None-Match: *`): load-bearing from phase 1.
    let cia_key = object_store::path::Path::from(format!(".preflight/{}", Uuid::new_v4()));
    let create_if_absent = match store
        .put_opts(
            &cia_key,
            PutPayload::from_static(b"a"),
            PutOptions::from(PutMode::Create),
        )
        .await
    {
        Ok(_) => match store
            .put_opts(
                &cia_key,
                PutPayload::from_static(b"b"),
                PutOptions::from(PutMode::Create),
            )
            .await
        {
            Err(object_store::Error::AlreadyExists { .. }) => Ok(()),
            Ok(_) => Err("backend accepted a second create over an existing object; \
                          create-if-absent is not enforced"
                .to_string()),
            Err(e) => Err(concise_os_error(&e)),
        },
        Err(e) => Err(concise_os_error(&e)),
    };
    let _ = store.delete(&cia_key).await;
    checks.push(PreflightCheck::new(
        "create-if-absent (If-None-Match)",
        true,
        create_if_absent,
    ));

    // Etag CAS (`If-Match`): required for lease renew/takeover, and so
    // for multi-node mounts. Single-node mounts degrade without it, so
    // it is reported but not treated as fatal for `fs create`.
    let etag_cas = match &written {
        Some(v) => match store
            .put_opts(
                &key,
                PutPayload::from_static(b"cas"),
                PutOptions::from(PutMode::Update(v.clone())),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(concise_os_error(&e)),
        },
        None => Err("skipped (initial write failed)".to_string()),
    };
    checks.push(PreflightCheck::new("etag CAS (If-Match)", false, etag_cas));

    // Delete: cleanup uses it everywhere (GC, lease release).
    let delete = store.delete(&key).await;
    checks.push(PreflightCheck::new(
        "delete object (DELETE)",
        true,
        delete.map_err(|e| concise_os_error(&e)),
    ));

    checks
}

/// `doctor` probe: verify which conditional-write primitives the
/// backend supports. Create-if-absent is required from phase 1;
/// etag CAS becomes load-bearing with leases (phase 3).
pub async fn probe_conditional_writes(store: &dyn ObjectStore) -> Result<Capabilities, Error> {
    let key = object_store::path::Path::from(format!(".doctor/{}", Uuid::new_v4()));
    // Create-if-absent.
    let first = store
        .put_opts(
            &key,
            PutPayload::from_static(b"a"),
            PutOptions::from(PutMode::Create),
        )
        .await?;
    let create_if_absent = match store
        .put_opts(
            &key,
            PutPayload::from_static(b"b"),
            PutOptions::from(PutMode::Create),
        )
        .await
    {
        Err(object_store::Error::AlreadyExists { .. }) => true,
        Ok(_) => false,
        Err(e) => {
            let _ = store.delete(&key).await;
            return Err(e.into());
        }
    };
    // CAS with the correct etag must succeed; with a stale etag it must fail.
    let update = PutOptions::from(PutMode::Update(object_store::UpdateVersion {
        e_tag: first.e_tag.clone(),
        version: first.version.clone(),
    }));
    let etag_cas = match store
        .put_opts(&key, PutPayload::from_static(b"c"), update)
        .await
    {
        Ok(_) => {
            let stale = PutOptions::from(PutMode::Update(object_store::UpdateVersion {
                e_tag: first.e_tag,
                version: first.version,
            }));
            matches!(
                store
                    .put_opts(&key, PutPayload::from_static(b"d"), stale)
                    .await,
                Err(object_store::Error::Precondition { .. })
            )
        }
        Err(object_store::Error::NotImplemented { .. }) => false,
        Err(e) => {
            let _ = store.delete(&key).await;
            return Err(e.into());
        }
    };
    let _ = store.delete(&key).await;
    Ok(Capabilities {
        create_if_absent,
        etag_cas,
    })
}

/// Conditional-write support reported by [`probe_conditional_writes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub create_if_absent: bool,
    pub etag_cas: bool,
}

/// One operation exercised by [`preflight`].
#[derive(Debug, Clone)]
pub struct PreflightCheck {
    /// Human-readable operation name (e.g. `"create-if-absent (If-None-Match)"`).
    pub name: &'static str,
    /// Whether a filesystem cannot be created without this operation.
    pub required: bool,
    /// `Ok(())` when the operation worked, `Err(reason)` with a concise
    /// explanation otherwise.
    pub outcome: Result<(), String>,
}

impl PreflightCheck {
    pub fn new(name: &'static str, required: bool, outcome: Result<(), String>) -> Self {
        Self {
            name,
            required,
            outcome,
        }
    }
}

/// Collapse a verbose `object_store` error (which embeds the full S3 XML
/// body) into a one-line reason suitable for a preflight report.
pub fn concise_os_error(e: &object_store::Error) -> String {
    match e {
        object_store::Error::NotImplemented { .. } => {
            "not supported by this S3 backend (server returned 501 Not Implemented)".to_string()
        }
        object_store::Error::NotFound { .. } => "object not found".to_string(),
        object_store::Error::Precondition { .. } => "precondition failed".to_string(),
        object_store::Error::AlreadyExists { .. } => "object already exists".to_string(),
        object_store::Error::Generic { source, .. } => {
            // Generic S3 errors wrap the transport error; some backends
            // report an unimplemented conditional header this way.
            let msg = source.to_string();
            if msg.contains("501") || msg.contains("NotImplemented") {
                "not supported by this S3 backend (server returned 501 Not Implemented)".to_string()
            } else {
                // Keep just the first line; the rest is an XML dump.
                msg.lines().next().unwrap_or("request failed").to_string()
            }
        }
        other => other
            .to_string()
            .lines()
            .next()
            .unwrap_or("request failed")
            .to_string(),
    }
}

/// Whether a `LIST` error says the bucket is absent: `object_store` keeps
/// a `LIST`'s status only in the error text (S3's `NoSuchBucket` code, or
/// a 404 status; a `LIST` cannot 404 for any other reason).
pub fn is_missing_bucket(e: &object_store::Error) -> bool {
    if matches!(e, object_store::Error::NotFound { .. }) {
        return true;
    }
    let mut text = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        text.push_str(&s.to_string());
        source = s.source();
    }
    text.contains("NoSuchBucket") || text.contains("status code: 404")
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;

    #[tokio::test]
    async fn preflight_passes_and_cleans_up_on_capable_backend() {
        let s = InMemory::new();
        let checks = preflight(&s).await;
        assert!(checks.iter().all(|c| c.outcome.is_ok()), "{checks:?}");
        let leftover = s
            .list(Some(&object_store::path::Path::from(".preflight")))
            .collect::<Vec<_>>()
            .await;
        assert!(leftover.is_empty(), "preflight left scratch objects behind");
    }

    #[tokio::test]
    async fn probe_reports_both_conditional_primitives() {
        let caps = probe_conditional_writes(&InMemory::new()).await.unwrap();
        assert!(caps.create_if_absent && caps.etag_cas);
    }

    #[test]
    fn a_missing_bucket_is_recognised() {
        let e = object_store::Error::NotFound {
            path: "x".into(),
            source: "gone".into(),
        };
        assert!(is_missing_bucket(&e));
        let e = object_store::Error::Generic {
            store: "S3",
            source: "other".into(),
        };
        assert!(!is_missing_bucket(&e));
    }
}
