//! The one place that decides what is written: gate, append, notify.
//!
//! A write use case does not touch the store. It fills a draft with the events
//! and stored texts it wants, and hands it to `Workspace::commit`, which does three
//! things in this order:
//!
//! 1. **Gate** (can stop the write). The secret scan of every text about to be
//!    stored, and the leak lint of every text going to the repo store: it may carry
//!    neither the alias or title of a private entity, nor the identifier of one
//!    unless it is written `private:<id>` (a reference in the front matter and one in
//!    the body are seen alike, and so are `publish`, `add` and every other path that
//!    writes a text there). The entities published by the same draft are not
//!    private to it. Nothing is written if either objects.
//!
//!    There is no check of a gate here: an approval cannot be put in a draft except
//!    from the [`gate::Passed`](crate::domain::gate::Passed) of a gate's check (the
//!    draft's `push_approval`), so by the time it is in the draft its check has
//!    passed.
//! 2. **Append.** Texts first, then events, so no event points at a text that is not
//!    there. Appending is idempotent: an event or text already present is success.
//! 3. **Notify** (cannot stop anything). The model is loaded again from the stores
//!    and every subscriber is told; a failure becomes a warning in the result. If the
//!    model cannot be loaded the warning says so, because the write itself happened.
//!
//! The draft also keeps the **heads**. An event lists every tip of its target's
//! stream as the writer saw it; when a draft writes several events to one stream, each
//! lists the one before it, so a batch is a chain and never a fork.
//!
//! ```text
//!   command (add, tag, approve_cs, ingest, ...)      Workspace, &mut self
//!      |
//!      |  load Model; decide; fill a Draft:
//!      |    events (each lists the tips it saw; an approval only
//!      |    from a gate's Passed), texts to store
//!      v
//!   +--------------------------- commit -------------------------------+
//!   | 1. GATE  (nothing written yet; the first objection ends it)      |
//!   |      a) secret scan     each text to be stored -> SecretFound    |
//!   |      b) leak lint       each text for the repo store, against    |
//!   |                         private aliases, titles and unmarked     |
//!   |                         identifiers             -> LeakFound     |
//!   |                                                                  |
//!   | 2. APPEND  (idempotent: "already there" is success)              |
//!   |      texts  -> store.put_object   first, so no event dangles     |
//!   |      events -> store.append       in draft order                 |
//!   |                                                                  |
//!   | 3. NOTIFY  (cannot stop or undo anything)                        |
//!   |      Model::load again, then each subscriber in turn:            |
//!   |        cache -> map -> [status md] -> [changelog md]             |
//!   |      a failing subscriber adds a SubscriberWarning               |
//!   +------------------------------+-----------------------------------+
//!                                  |
//!                                  v
//!                 Committed { events: ids in order, warnings }
//! ```
//!
//! Only texts (the stored objects) are scanned for secrets and for private names;
//! the fields of an event are not.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::{Actor, Timestamp};
use crate::domain::event::{Event, EventBody, EventId, RecordBody, StoreKind};
use crate::domain::gate::Passed;
use crate::domain::id::EntityId;
use crate::domain::lint::leak_lint;

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
    publishing: BTreeSet<EntityId>,
}

impl Draft {
    pub(crate) fn new(at: Timestamp) -> Self {
        Self {
            at,
            tips: BTreeMap::new(),
            events: Vec::new(),
            objects: Vec::new(),
            publishing: BTreeSet::new(),
        }
    }

    /// An empty draft with the same time.
    pub(crate) fn like(&self) -> Draft {
        Draft::new(self.at.clone())
    }

    /// Add a record to `target`'s stream in `store`, listing every tip the model
    /// saw, or the previous event of this draft.
    ///
    /// Anything but an approval: the body type has no variant for one, so this is
    /// not a way round a gate (see [`push_approval`](Self::push_approval)).
    pub(crate) fn push(
        &mut self,
        model: &Model,
        store: StoreKind,
        target: EntityId,
        body: RecordBody,
    ) -> Result<EventId, AppError> {
        self.append(model, store, target, body.into())
    }

    /// Add an approval to the stream of the entity it is about, in `store`, by `by`.
    ///
    /// The one way to record an approval, and it takes the proof that a gate's check
    /// passed: the attestation is the one the check built.
    pub(crate) fn push_approval(
        &mut self,
        model: &Model,
        store: StoreKind,
        passed: &Passed,
        by: Actor,
    ) -> Result<EventId, AppError> {
        self.append(
            model,
            store,
            passed.subject(),
            EventBody::Approved {
                by,
                attestation: passed.attestation().clone(),
            },
        )
    }

    fn append(
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
        let names = model.private_names(&draft.publishing);
        let ids = model.private_ids(&draft.publishing);
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
                let findings = leak_lint(text.as_str(), &names, &ids);
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
