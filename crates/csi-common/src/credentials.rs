// SPDX-License-Identifier: MPL-2.0
//! The credentials a CSI driver hands to a storage daemon, and the Secret
//! watch behind a "refreshing" credential source.
//!
//! This is the part that needs no control protocol: the Secret data type and
//! its key names, the fingerprint used to tell whether a daemon already has a
//! set of credentials, and the [`Refresher`] (one Kubernetes Secret watch per
//! Secret per process, with the last subscriber's release ending it and
//! dropping the bytes). The Kubernetes client end ([`KubeSecrets`]) is behind
//! the `kube-secrets` feature. Pushing the credentials to a daemon
//! (`unlock_params`, `follow_rotations`, the "awaiting unlock" refusal) is
//! specific to each driver's control protocol and stays with the driver.
//!
//! A watch is a list with a `metadata.name` field selector (so RBAC can
//! scope it to that one name, `resourceNames`) followed by a watch from its
//! resourceVersion, re-listed with backoff (1 s doubling to 30 s) whenever
//! the watch ends. Each change of the Secret's *data* (not of its metadata)
//! is published to the subscribers. A deleted Secret is logged and the last
//! bytes kept. Nothing here ever logs a value; errors name the Secret, never
//! its data.

use crate::params::SecretRef;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// A Secret's data, as the CSI request (`req.secrets`) or the API carries it.
pub type Secrets = BTreeMap<String, String>;

pub const ACCESS_KEY_ID: &str = "aws_access_key_id";
pub const SECRET_ACCESS_KEY: &str = "aws_secret_access_key";
pub const SESSION_TOKEN: &str = "aws_session_token";
pub const E2E_PASSPHRASE: &str = "e2e_passphrase";

/// A fingerprint of the credentials (the key pair, token and passphrase), to tell whether an
/// engine already has it. Kept in memory only, never logged.
pub fn fingerprint(secrets: &Secrets) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for key in [
        ACCESS_KEY_ID,
        SECRET_ACCESS_KEY,
        SESSION_TOKEN,
        E2E_PASSPHRASE,
    ] {
        h.update(key.as_bytes());
        h.update(&[0]);
        h.update(secrets.get(key).map(String::as_bytes).unwrap_or_default());
        h.update(&[0]);
    }
    *h.finalize().as_bytes()
}

/// What a watch reports.
pub enum SecretEvent {
    /// The Secret's data now.
    Changed(Secrets),
    Deleted,
}

/// Where Secrets are read and watched: the API server ([`KubeSecrets`]),
/// or a test double.
#[async_trait]
pub trait SecretSource: Send + Sync + 'static {
    /// The Secret's data (`None`: it does not exist) and the
    /// resourceVersion to watch from.
    async fn read(&self, secret: &SecretRef) -> Result<(Option<Secrets>, String), String>;
    /// Its changes after `version`, until the stream ends.
    async fn watch(
        &self,
        secret: &SecretRef,
        version: &str,
    ) -> Result<BoxStream<'static, Result<SecretEvent, String>>, String>;
}

/// [`SecretSource`] over the Kubernetes API, scoped to one name per
/// request (`fieldSelector=metadata.name=<name>`), which is what lets the
/// chart grant `get`/`list`/`watch` on exactly the watched Secrets.
#[cfg(feature = "kube-secrets")]
pub struct KubeSecrets {
    client: kube::Client,
}

#[cfg(feature = "kube-secrets")]
impl KubeSecrets {
    pub fn new(client: kube::Client) -> KubeSecrets {
        KubeSecrets { client }
    }

    fn api(&self, secret: &SecretRef) -> kube::Api<k8s_openapi::api::core::v1::Secret> {
        kube::Api::namespaced(self.client.clone(), &secret.namespace)
    }
}

#[cfg(feature = "kube-secrets")]
fn secret_data(s: &k8s_openapi::api::core::v1::Secret) -> Secrets {
    s.data
        .iter()
        .flatten()
        .map(|(k, v)| (k.clone(), String::from_utf8_lossy(&v.0).into_owned()))
        .collect()
}

#[cfg(feature = "kube-secrets")]
#[async_trait]
impl SecretSource for KubeSecrets {
    async fn read(&self, secret: &SecretRef) -> Result<(Option<Secrets>, String), String> {
        let list = self
            .api(secret)
            .list(
                &kube::api::ListParams::default().fields(&format!("metadata.name={}", secret.name)),
            )
            .await
            .map_err(|e| e.to_string())?;
        let version = list.metadata.resource_version.clone().unwrap_or_default();
        Ok((list.items.first().map(secret_data), version))
    }

    async fn watch(
        &self,
        secret: &SecretRef,
        version: &str,
    ) -> Result<BoxStream<'static, Result<SecretEvent, String>>, String> {
        use kube::api::WatchEvent;
        let params = kube::api::WatchParams::default()
            .fields(&format!("metadata.name={}", secret.name))
            .timeout(290);
        let stream = self
            .api(secret)
            .watch(&params, version)
            .await
            .map_err(|e| e.to_string())?;
        Ok(stream
            .filter_map(|event| async move {
                match event {
                    Ok(WatchEvent::Added(s)) | Ok(WatchEvent::Modified(s)) => {
                        Some(Ok(SecretEvent::Changed(secret_data(&s))))
                    }
                    Ok(WatchEvent::Deleted(_)) => Some(Ok(SecretEvent::Deleted)),
                    Ok(WatchEvent::Bookmark(_)) => None,
                    Ok(WatchEvent::Error(status)) => Some(Err(status.to_string())),
                    Err(e) => Some(Err(e.to_string())),
                }
            })
            .boxed())
    }
}

/// The latest data of a watched Secret.
pub type Latest = watch::Receiver<Option<Arc<Secrets>>>;

/// One running watch and the units subscribed to it.
struct Watch {
    latest: Latest,
    subscribers: usize,
    task: tokio::task::AbortHandle,
}

/// One watch per Secret, shared by every subscriber in the process
/// (module docs); the last [`Refresher::release`] ends it and drops the
/// bytes it held.
pub struct Refresher {
    source: Arc<dyn SecretSource>,
    watches: Mutex<HashMap<SecretRef, Watch>>,
    /// The first re-list delay after a watch ends; tests shorten it.
    backoff: Duration,
}

impl Refresher {
    pub fn new(source: Arc<dyn SecretSource>) -> Arc<Refresher> {
        Arc::new(Refresher {
            source,
            watches: Mutex::default(),
            backoff: Duration::from_secs(1),
        })
    }

    /// A refresher with a chosen first re-list delay (tests shorten it).
    pub fn with_backoff(source: Arc<dyn SecretSource>, backoff: Duration) -> Arc<Refresher> {
        Arc::new(Refresher {
            source,
            watches: Mutex::default(),
            backoff,
        })
    }

    /// The latest data of `secret`, watched from the first call on; counts
    /// one subscriber, to be given back with [`Self::release`].
    pub fn subscribe(&self, secret: &SecretRef) -> Latest {
        let mut watches = self.watches.lock().unwrap();
        if let Some(watch) = watches.get_mut(secret) {
            watch.subscribers += 1;
            return watch.latest.clone();
        }
        let (tx, rx) = watch::channel(None);
        let task = tokio::spawn(run_watch(
            self.source.clone(),
            secret.clone(),
            tx,
            self.backoff,
        ));
        watches.insert(
            secret.clone(),
            Watch {
                latest: rx.clone(),
                subscribers: 1,
                task: task.abort_handle(),
            },
        );
        rx
    }

    /// Give back one [`Self::subscribe`]. The last one ends the watch and
    /// drops the Secret's bytes with it.
    pub fn release(&self, secret: &SecretRef) {
        let mut watches = self.watches.lock().unwrap();
        let Some(watch) = watches.get_mut(secret) else {
            return;
        };
        watch.subscribers = watch.subscribers.saturating_sub(1);
        if watch.subscribers == 0 {
            if let Some(watch) = watches.remove(secret) {
                watch.task.abort();
            }
        }
    }

    /// [`Self::subscribe`], waiting up to `wait` for the first read; the
    /// subscription is given back on return, so a watch only this call
    /// wanted ends with it (callers that mean to keep one subscribe
    /// first).
    pub async fn current(&self, secret: &SecretRef, wait: Duration) -> Option<Arc<Secrets>> {
        let mut latest = self.subscribe(secret);
        let _ = tokio::time::timeout(wait, latest.wait_for(Option::is_some)).await;
        let current = latest.borrow().clone();
        drop(latest);
        self.release(secret);
        current
    }
}

/// Publish `data` unless its credentials are what subscribers have.
fn publish(tx: &watch::Sender<Option<Arc<Secrets>>>, data: Secrets) {
    tx.send_if_modified(|have| {
        let same = have
            .as_ref()
            .is_some_and(|h| fingerprint(h) == fingerprint(&data));
        if !same {
            *have = Some(Arc::new(data));
        }
        !same
    });
}

async fn run_watch(
    source: Arc<dyn SecretSource>,
    secret: SecretRef,
    tx: watch::Sender<Option<Arc<Secrets>>>,
    first_backoff: Duration,
) {
    let mut backoff = first_backoff;
    loop {
        match source.read(&secret).await {
            Ok((data, version)) => {
                match data {
                    Some(data) => publish(&tx, data),
                    None => tracing::warn!(%secret,
                        "the watched credentials Secret does not exist; engines keep what they have"),
                }
                match source.watch(&secret, &version).await {
                    Ok(mut events) => {
                        backoff = first_backoff;
                        while let Some(event) = events.next().await {
                            match event {
                                Ok(SecretEvent::Changed(data)) => publish(&tx, data),
                                Ok(SecretEvent::Deleted) => tracing::warn!(%secret,
                                    "the watched credentials Secret was deleted; engines keep what they have"),
                                Err(e) => {
                                    tracing::debug!(%secret, error = %e, "the Secret watch ended");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!(%secret, error = %e, "watching a credentials Secret"),
                }
            }
            Err(e) => tracing::warn!(%secret, error = %e, "reading a credentials Secret"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// A [`SecretSource`] double for this crate's tests.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use super::*;

    /// A Secret in memory: `set` changes it and wakes the watch; `end`
    /// ends the current watch stream (as the API server does).
    #[derive(Default)]
    pub struct FakeSecrets {
        pub state: Mutex<(Option<Secrets>, u64)>,
        pub events:
            Mutex<Vec<futures::channel::mpsc::UnboundedSender<Result<SecretEvent, String>>>>,
        pub reads: std::sync::atomic::AtomicU64,
    }

    impl FakeSecrets {
        pub fn set(&self, data: Option<Secrets>) {
            let mut st = self.state.lock().unwrap();
            st.0 = data.clone();
            st.1 += 1;
            for tx in self.events.lock().unwrap().iter() {
                let _ = tx.unbounded_send(Ok(match &data {
                    Some(d) => SecretEvent::Changed(d.clone()),
                    None => SecretEvent::Deleted,
                }));
            }
        }

        pub fn end(&self) {
            for tx in self.events.lock().unwrap().drain(..) {
                let _ = tx.unbounded_send(Err("watch closed".into()));
            }
        }
    }

    #[async_trait]
    impl SecretSource for Arc<FakeSecrets> {
        async fn read(&self, _: &SecretRef) -> Result<(Option<Secrets>, String), String> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let st = self.state.lock().unwrap();
            Ok((st.0.clone(), st.1.to_string()))
        }

        async fn watch(
            &self,
            _: &SecretRef,
            _: &str,
        ) -> Result<BoxStream<'static, Result<SecretEvent, String>>, String> {
            let (tx, rx) = futures::channel::mpsc::unbounded();
            self.events.lock().unwrap().push(tx);
            Ok(rx.boxed())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeSecrets;
    use super::*;

    fn secrets(kv: &[(&str, &str)]) -> Secrets {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_fingerprint_follows_the_credentials_not_unrelated_keys() {
        let full = secrets(&[
            (ACCESS_KEY_ID, "id"),
            (SECRET_ACCESS_KEY, "key"),
            (SESSION_TOKEN, "tok"),
            (E2E_PASSPHRASE, "pw"),
            ("unrelated", "x"),
        ]);
        let mut other = full.clone();
        other.insert("unrelated".into(), "y".into());
        assert_eq!(fingerprint(&full), fingerprint(&other));
        other.insert(SECRET_ACCESS_KEY.into(), "rotated".into());
        assert_ne!(fingerprint(&full), fingerprint(&other));
    }

    async fn next_value(latest: &mut Latest) -> Arc<Secrets> {
        tokio::time::timeout(Duration::from_secs(10), latest.changed())
            .await
            .expect("a change within 10 s")
            .unwrap();
        latest.borrow().clone().unwrap()
    }

    /// The `Refreshing(callback)` source: every change of the Secret's
    /// credentials reaches a subscriber, once; a re-list after the watch
    /// ends republishes nothing unchanged; a deletion keeps the last bytes;
    /// one watch serves every subscriber of one Secret.
    #[tokio::test]
    async fn a_watched_secret_publishes_each_rotation_once() {
        let fake = Arc::new(FakeSecrets::default());
        fake.set(Some(secrets(&[
            (ACCESS_KEY_ID, "A"),
            (SECRET_ACCESS_KEY, "a"),
        ])));
        let refresher = Refresher::with_backoff(Arc::new(fake.clone()), Duration::from_millis(10));
        let r = SecretRef {
            namespace: "ns".into(),
            name: "creds".into(),
        };
        let mut latest = refresher.subscribe(&r);
        let first = refresher
            .current(&r, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(first[ACCESS_KEY_ID], "A");
        latest.borrow_and_update();

        fake.set(Some(secrets(&[
            (ACCESS_KEY_ID, "B"),
            (SECRET_ACCESS_KEY, "b"),
        ])));
        assert_eq!(next_value(&mut latest).await[ACCESS_KEY_ID], "B");

        // The watch ends; the re-list finds nothing new and publishes
        // nothing, and the new watch carries the next rotation.
        let reads = fake.reads.load(std::sync::atomic::Ordering::SeqCst);
        fake.end();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while fake.reads.load(std::sync::atomic::Ordering::SeqCst) == reads {
            assert!(std::time::Instant::now() < deadline, "no re-list");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        while fake.events.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            !latest.has_changed().unwrap(),
            "an unchanged re-list republished"
        );
        fake.set(Some(secrets(&[
            (ACCESS_KEY_ID, "C"),
            (SECRET_ACCESS_KEY, "c"),
        ])));
        assert_eq!(next_value(&mut latest).await[ACCESS_KEY_ID], "C");

        // Deleted: the last bytes stay.
        fake.set(None);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!latest.has_changed().unwrap());
        assert_eq!(latest.borrow().as_ref().unwrap()[ACCESS_KEY_ID], "C");

        // A second subscriber shares the one watch.
        let other = refresher.subscribe(&r);
        assert_eq!(other.borrow().as_ref().unwrap()[ACCESS_KEY_ID], "C");
        assert_eq!(fake.events.lock().unwrap().len(), 1);
    }

    /// The last subscriber to give a Secret back ends its watch: the API
    /// stream is dropped and the bytes are gone; earlier ones leave it
    /// running; a later subscriber starts afresh.
    #[tokio::test]
    async fn the_last_release_ends_the_watch_and_drops_the_bytes() {
        let fake = Arc::new(FakeSecrets::default());
        fake.set(Some(secrets(&[
            (ACCESS_KEY_ID, "A"),
            (SECRET_ACCESS_KEY, "a"),
        ])));
        let refresher = Refresher::with_backoff(Arc::new(fake.clone()), Duration::from_millis(10));
        let r = SecretRef {
            namespace: "ns".into(),
            name: "creds".into(),
        };
        let one = refresher.subscribe(&r);
        let mut two = refresher.subscribe(&r);
        assert!(refresher
            .current(&r, Duration::from_secs(10))
            .await
            .is_some());
        // `current` gave its own subscription back: the watch still runs.
        assert_eq!(fake.events.lock().unwrap().len(), 1);
        two.wait_for(Option::is_some).await.unwrap();
        two.borrow_and_update();
        drop(one);
        refresher.release(&r);
        fake.set(Some(secrets(&[
            (ACCESS_KEY_ID, "B"),
            (SECRET_ACCESS_KEY, "b"),
        ])));
        assert_eq!(next_value(&mut two).await[ACCESS_KEY_ID], "B");

        refresher.release(&r);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !fake.events.lock().unwrap().iter().all(|tx| tx.is_closed()) {
            assert!(
                std::time::Instant::now() < deadline,
                "the stream stayed open"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(refresher.watches.lock().unwrap().is_empty());
        assert!(
            two.changed().await.is_err(),
            "the sender, and the bytes it held, are gone"
        );
        // A new subscriber starts a new watch from nothing.
        let _fresh = refresher.subscribe(&r);
        assert_eq!(refresher.watches.lock().unwrap().len(), 1);
        refresher.release(&r);
        assert!(refresher.watches.lock().unwrap().is_empty());
    }
}
