//! What happens after a write: subscribers.
//!
//! A subscriber is told which events were just recorded and gets the model as it
//! is afterwards. It cannot stop anything: the gate (see [`commit`](super::commit))
//! is where a write is refused, and a subscriber that fails only produces a
//! [`SubscriberWarning`] that the use case returns next to its result. The same
//! mechanism runs the built-in work (so it is exercised from the first day) and is
//! where a user's own hook would plug in later. There is no hook on a file change or
//! on a command: the entry is the event, so a change made by the command line, the
//! tool server or a direct edit (found by ingest) is heard alike.
//!
//! | subscriber | does |
//! |---|---|
//! | [`MapExporter`] | rebuilds the map (identifiers, relations, tags) of each store the events touched; no derived state in it, so it changes only when relations or tags do |
//! | [`CacheUpdater`] | refreshes the cache rows of the documents the events touched |
//! | `StatusMdExporter` | (optional) rebuilds the status Markdown of the repo store, leaving private entities out |
//! | `ChangelogMdExporter` | (optional) rebuilds the changelog Markdown of the repo store |
//!
//! There is no `dyn`: the built-in subscribers are variants of [`Builtin`], and the
//! trait is generic over the ports so that a subscriber borrows them for the call.

use std::collections::BTreeSet;
use std::error::Error;
use std::future::Future;

use thiserror::Error;

use crate::domain::event::{Event, EventBody, StoreKind};
use crate::domain::id::EntityId;

use super::config::Config;
use super::error::AppError;
use super::model::Model;
use super::ports::{CacheEntry, ExportDoc, IndexCachePort, PortName, Ports, StorePort};
use super::projection::{ChangelogDoc, MapDoc};
use super::view::{StatusReport, context_line};

/// Why a subscriber failed. The cause is kept.
#[derive(Debug, Error)]
#[error("a subscriber failed")]
pub struct SubscriberError {
    #[source]
    source: Box<dyn Error + Send + Sync>,
}

impl SubscriberError {
    /// Wrap the cause of a failure.
    pub fn new<E: Error + Send + Sync + 'static>(source: E) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}

impl From<AppError> for SubscriberError {
    fn from(error: AppError) -> Self {
        Self::new(error)
    }
}

/// A subscriber's failure, returned next to the result of the command that
/// caused it.
#[derive(Debug, Error)]
#[error("the subscriber `{name}` failed")]
pub struct SubscriberWarning {
    name: &'static str,
    #[source]
    error: SubscriberError,
}

impl SubscriberWarning {
    pub(crate) fn new(name: &'static str, error: SubscriberError) -> Self {
        Self { name, error }
    }

    /// Which subscriber.
    pub fn name(&self) -> &'static str {
        self.name
    }
}

/// Something told about each write after it is recorded.
pub trait Subscriber<P: Ports>: Send + Sync {
    /// A short name for warnings.
    fn name(&self) -> &'static str;

    /// The events just recorded (none for a rebuild) and the model as it is
    /// now.
    fn on_committed(
        &self,
        ports: &P,
        config: &Config,
        events: &[Event],
        model: &Model,
    ) -> impl Future<Output = Result<(), SubscriberError>> + Send;
}

/// Writes the map of each store the events touched.
#[derive(Debug, Clone, Copy, Default)]
pub struct MapExporter;

/// The stores the events were recorded in; with no events (a rebuild), every
/// store that has any.
fn touched_stores(events: &[Event], model: &Model) -> BTreeSet<StoreKind> {
    if events.is_empty() {
        return [StoreKind::Repo, StoreKind::Private]
            .into_iter()
            .filter(|s| !model.events(*s).is_empty())
            .collect();
    }
    events
        .iter()
        .filter_map(|e| model.store_of_event(&e.id()))
        .collect()
}

impl<P: Ports> Subscriber<P> for MapExporter {
    fn name(&self) -> &'static str {
        "map"
    }

    async fn on_committed(
        &self,
        ports: &P,
        _config: &Config,
        events: &[Event],
        model: &Model,
    ) -> Result<(), SubscriberError> {
        for store in touched_stores(events, model) {
            let map = MapDoc::build(model, store);
            ports
                .store()
                .write_export(store, &ExportDoc::Map(&map))
                .await
                .map_err(AppError::port(PortName::Store))?;
        }
        Ok(())
    }
}

/// Refreshes the cache rows of the documents the events touched.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheUpdater;

impl<P: Ports> Subscriber<P> for CacheUpdater {
    fn name(&self) -> &'static str {
        "cache"
    }

    async fn on_committed(
        &self,
        ports: &P,
        _config: &Config,
        events: &[Event],
        model: &Model,
    ) -> Result<(), SubscriberError> {
        let mut touched: BTreeSet<EntityId> = BTreeSet::new();
        let mut vacated = Vec::new();
        for event in events {
            touched.insert(model.canonical(event.target()));
            if let EventBody::Moved { old, .. } = event.body() {
                vacated.push(old.clone());
            }
        }
        let mut entries = Vec::new();
        for id in touched {
            let Some(doc) = model.doc(id) else { continue };
            let (Some(present), Some(stat)) = (doc.present(), model.stats().get(doc.path())) else {
                continue;
            };
            entries.push(CacheEntry::new(
                stat.clone(),
                *present.raw(),
                present.content().search_text(),
            ));
        }
        vacated.retain(|p| !entries.iter().any(|e| e.path() == p));
        if !vacated.is_empty() {
            ports
                .cache()
                .remove(&vacated)
                .await
                .map_err(AppError::port(PortName::IndexCache))?;
        }
        if !entries.is_empty() {
            ports
                .cache()
                .upsert(&entries)
                .await
                .map_err(AppError::port(PortName::IndexCache))?;
        }
        Ok(())
    }
}

/// Writes the status Markdown of the repo store (optional).
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusMdExporter;

impl<P: Ports> Subscriber<P> for StatusMdExporter {
    fn name(&self) -> &'static str {
        "status-md"
    }

    async fn on_committed(
        &self,
        ports: &P,
        config: &Config,
        _events: &[Event],
        model: &Model,
    ) -> Result<(), SubscriberError> {
        let context = context_line(ports, config).await?;
        let status = StatusReport::build(model, context).public_part();
        ports
            .store()
            .write_export(StoreKind::Repo, &ExportDoc::Status(&status))
            .await
            .map_err(AppError::port(PortName::Store))?;
        Ok(())
    }
}

/// Writes the changelog Markdown of the repo store (optional).
#[derive(Debug, Clone, Copy, Default)]
pub struct ChangelogMdExporter;

impl<P: Ports> Subscriber<P> for ChangelogMdExporter {
    fn name(&self) -> &'static str {
        "changelog-md"
    }

    async fn on_committed(
        &self,
        ports: &P,
        _config: &Config,
        _events: &[Event],
        model: &Model,
    ) -> Result<(), SubscriberError> {
        let changelog = ChangelogDoc::build(model, StoreKind::Repo);
        ports
            .store()
            .write_export(StoreKind::Repo, &ExportDoc::Changelog(&changelog))
            .await
            .map_err(AppError::port(PortName::Store))?;
        Ok(())
    }
}

/// The built-in subscribers, as one type, so no `dyn` is needed to hold them.
#[derive(Debug, Clone, Copy)]
pub enum Builtin {
    /// [`MapExporter`].
    Map(MapExporter),
    /// [`CacheUpdater`].
    Cache(CacheUpdater),
    /// [`StatusMdExporter`].
    StatusMd(StatusMdExporter),
    /// [`ChangelogMdExporter`].
    ChangelogMd(ChangelogMdExporter),
}

impl Builtin {
    /// The subscribers a configuration asks for: the map and the cache always,
    /// the Markdown files if enabled.
    pub fn from_config(config: &Config) -> Vec<Builtin> {
        let mut out = vec![Builtin::Cache(CacheUpdater), Builtin::Map(MapExporter)];
        if config.export().status_md() {
            out.push(Builtin::StatusMd(StatusMdExporter));
        }
        if config.export().changelog_md() {
            out.push(Builtin::ChangelogMd(ChangelogMdExporter));
        }
        out
    }
}

impl<P: Ports> Subscriber<P> for Builtin {
    fn name(&self) -> &'static str {
        match self {
            Self::Map(s) => Subscriber::<P>::name(s),
            Self::Cache(s) => Subscriber::<P>::name(s),
            Self::StatusMd(s) => Subscriber::<P>::name(s),
            Self::ChangelogMd(s) => Subscriber::<P>::name(s),
        }
    }

    async fn on_committed(
        &self,
        ports: &P,
        config: &Config,
        events: &[Event],
        model: &Model,
    ) -> Result<(), SubscriberError> {
        match self {
            Self::Map(s) => s.on_committed(ports, config, events, model).await,
            Self::Cache(s) => s.on_committed(ports, config, events, model).await,
            Self::StatusMd(s) => s.on_committed(ports, config, events, model).await,
            Self::ChangelogMd(s) => s.on_committed(ports, config, events, model).await,
        }
    }
}
