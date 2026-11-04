//! The one place that decides what is written: gate, append, notify.
//!
//! A write use case does not touch the store. It fills a draft with the events
//! and stored texts it wants, and hands it to `Workspace::commit`, which does three
//! things in this order:
//!
//! 1. **Gate** (can stop the write). The secret scan of every text about to be
//!    stored; the leak lint of every text going to the repo store, against the
//!    aliases and titles of the private entities; and the machine check of every
//!    approval, whose violations the use case passed along. Nothing is written if any
//!    of them objects.
//! 2. **Append.** Texts first, then events, so no event points at a text that is not
//!    there. Appending is idempotent: an event or text already present is success.
//! 3. **Notify** (cannot stop anything). The model is loaded again from the stores
//!    and every subscriber is told; a failure becomes a warning in the result. If the
//!    model cannot be loaded the warning says so, because the write itself happened.
//!
//! The draft also keeps the **heads**. An event lists every tip of its target's
//! stream as the writer saw it; when a draft writes several events to one stream, each
//! lists the one before it, so a batch is a chain and never a fork.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::Timestamp;
use crate::domain::approval::Gate;
use crate::domain::event::{Event, EventBody, EventId, StoreKind};
use crate::domain::id::EntityId;
use crate::domain::lint::{CheckViolation, leak_lint};

use super::Workspace;
use super::error::AppError;
use super::model::Model;
use super::ports::{ClockPort, PortName, Ports, SecretScanPort, StorePort};
use super::raw::NormalizedText;
use super::subscriber::{Subscriber, SubscriberError, SubscriberWarning};

/// What a write use case wants recorded, before the gate has looked at it.
#[derive(Debug)]
pub(crate) struct Draft {
    at: Timestamp,
    tips: BTreeMap<(StoreKind, EntityId), BTreeSet<EventId>>,
    events: Vec<(StoreKind, Event)>,
    objects: Vec<(StoreKind, NormalizedText)>,
    checks: Vec<(Gate, Vec<CheckViolation>)>,
    publishing: BTreeSet<EntityId>,
}

impl Draft {
    pub(crate) fn new(at: Timestamp) -> Self {
        Self {
            at,
            tips: BTreeMap::new(),
            events: Vec::new(),
            objects: Vec::new(),
            checks: Vec::new(),
            publishing: BTreeSet::new(),
        }
    }

    /// An empty draft with the same time.
    pub(crate) fn like(&self) -> Draft {
        Draft::new(self.at.clone())
    }

    /// Add an event to `target`'s stream in `store`, listing every tip the model
    /// saw, or the previous event of this draft.
    pub(crate) fn push(
        &mut self,
        model: &Model,
        store: StoreKind,
        target: EntityId,
        body: EventBody,
    ) -> Result<EventId, AppError> {
        let heads = match self.tips.get(&(store, target)) {
            Some(tips) => tips.clone(),
            None => model.tips_in(store, target),
        };
        let event = Event::new(target, heads, self.at.clone(), body)?;
        let id = event.id();
        self.tips.insert((store, target), BTreeSet::from([id]));
        self.events.push((store, event));
        Ok(id)
    }

    /// Add a text to store under its `raw` hash.
    pub(crate) fn object(&mut self, store: StoreKind, text: NormalizedText) {
        if !self
            .objects
            .iter()
            .any(|(s, t)| *s == store && t.raw_hash() == text.raw_hash())
        {
            self.objects.push((store, text));
        }
    }

    /// Record the machine check of a gate; any violation stops the commit.
    pub(crate) fn require(&mut self, gate: Gate, violations: Vec<CheckViolation>) {
        self.checks.push((gate, violations));
    }

    /// Mark an entity as being published by this draft: its own alias and title
    /// are no longer secret.
    pub(crate) fn publishing(&mut self, id: EntityId) {
        self.publishing.insert(id);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty() && self.objects.is_empty()
    }

    /// Join the draft of another entity into this one. The streams of two
    /// drafts must be different, which holds for drafts of different files.
    pub(crate) fn merge(&mut self, other: Draft) {
        self.tips.extend(other.tips);
        self.events.extend(other.events);
        for (store, text) in other.objects {
            self.object(store, text);
        }
        self.checks.extend(other.checks);
        self.publishing.extend(other.publishing);
    }
}

/// What a commit wrote and what went wrong after it.
#[derive(Debug)]
pub struct Committed {
    events: Vec<EventId>,
    warnings: Vec<SubscriberWarning>,
}

impl Committed {
    /// A write that had nothing to record.
    pub(crate) fn nothing() -> Self {
        Self {
            events: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// The events of the write, in order (including any that were already there).
    pub fn events(&self) -> &[EventId] {
        &self.events
    }

    /// The subscribers that failed. The write happened regardless.
    pub fn warnings(&self) -> &[SubscriberWarning] {
        &self.warnings
    }

    /// Take the warnings out, for a use case that adds its own result.
    pub fn into_warnings(self) -> Vec<SubscriberWarning> {
        self.warnings
    }
}

impl<P: Ports> Workspace<P> {
    /// The time for a draft.
    pub(crate) async fn draft(&self) -> Result<Draft, AppError> {
        let at = self
            .ports
            .clock()
            .now()
            .await
            .map_err(AppError::port(PortName::Clock))?;
        Ok(Draft::new(at))
    }

    /// Step 1: may this be written? Nothing is written here.
    pub(crate) async fn gate(&self, model: &Model, draft: &Draft) -> Result<(), AppError> {
        for (gate, violations) in &draft.checks {
            if !violations.is_empty() {
                return Err(AppError::CheckFailed {
                    gate: *gate,
                    violations: violations.clone(),
                });
            }
        }
        let names = model.private_names(&draft.publishing);
        for (store, text) in &draft.objects {
            let hits = self
                .ports
                .secret()
                .scan(text)
                .await
                .map_err(AppError::port(PortName::SecretScan))?;
            if !hits.is_empty() {
                return Err(AppError::SecretFound { hits });
            }
            if *store == StoreKind::Repo {
                let findings = leak_lint(text.as_str(), &names);
                if !findings.is_empty() {
                    return Err(AppError::LeakFound { findings });
                }
            }
        }
        Ok(())
    }

    /// Gate, append, notify.
    pub(crate) async fn commit(&self, model: &Model, draft: Draft) -> Result<Committed, AppError> {
        self.gate(model, &draft).await?;
        for (store, text) in &draft.objects {
            self.ports
                .store()
                .put_object(*store, &text.raw_hash(), text)
                .await
                .map_err(AppError::port(PortName::Store))?;
        }
        for (store, event) in &draft.events {
            self.ports
                .store()
                .append(*store, event)
                .await
                .map_err(AppError::port(PortName::Store))?;
        }
        let events: Vec<Event> = draft.events.into_iter().map(|(_, e)| e).collect();
        let warnings = self.notify(&events).await;
        Ok(Committed {
            events: events.iter().map(Event::id).collect(),
            warnings,
        })
    }

    /// Step 3: tell every subscriber. Never fails; collects the failures.
    pub(crate) async fn notify(&self, events: &[Event]) -> Vec<SubscriberWarning> {
        let model = match Model::load(&self.ports, &self.config).await {
            Ok(model) => model,
            Err(error) => {
                return vec![SubscriberWarning::new("model", SubscriberError::new(error))];
            }
        };
        let mut warnings = Vec::new();
        for subscriber in &self.subscribers {
            if let Err(error) = subscriber
                .on_committed(&self.ports, &self.config, events, &model)
                .await
            {
                warnings.push(SubscriberWarning::new(
                    Subscriber::<P>::name(subscriber),
                    error,
                ));
            }
        }
        warnings
    }
}
