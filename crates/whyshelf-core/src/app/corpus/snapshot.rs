//! The read every use case of the corpus starts from.
//!
//! A [`Snapshot`] is what the ports say now, taken once and put through the domain's
//! two entries: the files and the ledgers go to [`Corpus::build`], and the corpus, the
//! ledgers and the person's keys go to [`evaluate_corpus`].
//!
//! ```text
//!   DocumentSource::list ----+                         +--> Corpus ----------+
//!   RevisionFactsPort::      |  Location rebuilt with  |                     |
//!     git_place per file ----+--> what Git makes of ---+                     +--> CorpusEvaluation
//!   LedgerRead (both sides) -----> Ledgers ------------+---------------------+
//!   KeySource -------------------> PinKeyring ---------+---------------------+
//!   (the text of each file is kept, for the secret scan of what is written)
//! ```
//!
//! It is built from [`CorpusRead`] and nothing else, so a use case that only reads
//! (status, check) can build it and has no port to write with.
//!
//! # What is kept and not lost
//!
//! The paths the listing found and could not name ([`unreadable`](Snapshot::unreadable))
//! and the records the ledgers could not decode ([`corrupt`](Snapshot::corrupt)) are
//! kept beside the corpus: not being in the corpus is not the same as not being on the
//! disk. A person's keys that cannot be looked for here are an empty keyring: with no
//! key, a keyed pin is Unknown and a private upstream cannot be pinned, which is what
//! "there is no place to look" comes to.
//!
//! # Names
//!
//! A reference given as an argument is looked up in the corpus's own index
//! ([`Snapshot::resolve`]). The histories that explain a rename (the aliases a
//! document had, the numbers an item had) are not read by any port yet, so they are
//! passed as `Unknown`: a name that matches nothing says so with an unknown hint and
//! never with "no earlier name fits".

use std::collections::BTreeMap;

use crate::app::commit::record::ChainError;
use crate::app::config::Config;
use crate::app::corpus::flow::{Flow, finish};
use crate::app::corpus::name::outcome_of;
use crate::app::error::AppError;
use crate::app::outcome::{Outcome, Stopped};
use crate::app::ports::{
    CorpusRead, CorruptRecord, DocumentSource, KeySource, LedgerRead, PortName, RevisionFactsPort,
    UnreadablePath,
};
use crate::app::raw::NormalizedText;
use crate::domain::boundary::PrivateRoot;
use crate::domain::corpus::{Corpus, FileFacts, Location, Visibility};
use crate::domain::corpus_eval::{CorpusEvaluation, CorpusInput, evaluate_corpus};
use crate::domain::gate::UpstreamPins;
use crate::domain::hash::TextHash;
use crate::domain::keyring::PinKeyring;
use crate::domain::knowable::Knowable;
use crate::domain::ledger::Ledgers;
use crate::domain::naming::{Named, Ref, Resolution, Resolver};
use crate::domain::policy::GatePolicy;

/// The corpus, the ledgers and the state of everything in them, as the ports show them
/// now. See the module documentation.
#[derive(Debug, Clone)]
pub struct Snapshot {
    corpus: Corpus,
    ledgers: Ledgers,
    keyring: PinKeyring,
    evaluation: CorpusEvaluation,
    texts: BTreeMap<TextHash, NormalizedText>,
    private_root: PrivateRoot,
    unreadable: Vec<UnreadablePath>,
    corrupt: Vec<(Visibility, CorruptRecord)>,
    policy: GatePolicy,
}

impl Snapshot {
    /// Read everything and evaluate it, under `policy` (what the project demands of a
    /// Spec at CS).
    pub async fn read<P: CorpusRead>(ports: &P, policy: GatePolicy) -> Result<Self, AppError> {
        let listing = ports
            .documents()
            .list()
            .await
            .map_err(AppError::port(PortName::Documents))?;
        let private_root = ports
            .documents()
            .private_root()
            .await
            .map_err(AppError::port(PortName::Documents))?;
        let (files, unreadable) = listing.into_parts();

        let mut readings = Vec::with_capacity(files.len());
        let mut texts = BTreeMap::new();
        for (facts, raw) in files {
            let place = ports
                .facts()
                .git_place(facts.location().visibility(), facts.location().path())
                .await
                .map_err(AppError::port(PortName::RevisionFacts))?;
            let located = FileFacts::new(
                Location::new(
                    facts.location().visibility(),
                    facts.location().path().clone(),
                    place,
                ),
                facts.bytes(),
            );
            let (reading, text) = raw.into_parts();
            texts.insert(text.text_hash(), text);
            readings.push((located, reading));
        }

        let public = ports
            .ledger()
            .read(Visibility::Public)
            .await
            .map_err(AppError::port(PortName::Ledger))?;
        let private = ports
            .ledger()
            .read(Visibility::Private)
            .await
            .map_err(AppError::port(PortName::Ledger))?;
        let ledgers = Ledgers::new(public.ledger(), private.ledger());
        let corrupt = public
            .corrupt()
            .iter()
            .map(|record| (Visibility::Public, *record))
            .chain(
                private
                    .corrupt()
                    .iter()
                    .map(|record| (Visibility::Private, *record)),
            )
            .collect();

        let keyring = match ports
            .key_source()
            .keys()
            .await
            .map_err(AppError::port(PortName::Keys))?
        {
            Knowable::Known(keyring) => keyring,
            Knowable::Unknown => PinKeyring::empty(),
        };

        let corpus = Corpus::build(readings, &ledgers.published_docs());
        let evaluation = evaluate_corpus(CorpusInput {
            corpus: &corpus,
            ledgers: &ledgers,
            keyring: &keyring,
            policy,
        });
        Ok(Self {
            corpus,
            ledgers,
            keyring,
            evaluation,
            texts,
            private_root,
            unreadable,
            corrupt,
            policy,
        })
    }

    /// The documents as the files say they are.
    pub fn corpus(&self) -> &Corpus {
        &self.corpus
    }

    /// The judgements, public and private.
    pub fn ledgers(&self) -> &Ledgers {
        &self.ledgers
    }

    /// The state of every document and item.
    pub fn evaluation(&self) -> &CorpusEvaluation {
        &self.evaluation
    }

    /// The person's keys (empty when there are none, or no place to look for them).
    pub fn keyring(&self) -> &PinKeyring {
        &self.keyring
    }

    /// What the project demands of a Spec at CS.
    pub fn policy(&self) -> GatePolicy {
        self.policy
    }

    /// The private documents, and the keys, a gate pins its upstreams with.
    pub fn pins(&self) -> UpstreamPins<'_> {
        self.evaluation.pins(&self.keyring)
    }

    /// Whether this person has a private root, and whether it could be read.
    pub fn private_root(&self) -> PrivateRoot {
        self.private_root
    }

    /// The paths the listing found and could not name.
    pub fn unreadable(&self) -> &[UnreadablePath] {
        &self.unreadable
    }

    /// The records a ledger holds and could not decode, with the side they are on.
    pub fn corrupt(&self) -> &[(Visibility, CorruptRecord)] {
        &self.corrupt
    }

    /// Look a reference up in the corpus. The one resolver of the use cases.
    pub fn resolve(&self, query: &Ref) -> Resolution<Named> {
        Resolver::new(self.corpus.index(), Knowable::Unknown, Knowable::Unknown).resolve(query)
    }

    /// What `query` names, or the outcome that says it names nothing one
    /// ([`outcome_of`]).
    pub fn named(&self, query: &Ref) -> Result<Named, Stopped> {
        outcome_of(query, self.resolve(query))
    }

    /// The normalized text of the version with this hash, as it was read.
    ///
    /// A gate is told the hash of the text it read, and the text is kept for each file
    /// of the listing; a hash that no file has is a mistake of the code that asks.
    pub fn text(&self, hash: TextHash) -> Result<&NormalizedText, ChainError> {
        self.texts
            .get(&hash)
            .ok_or(ChainError::TextNotRead { text: hash })
    }
}

/// What there is: every document and item with its state, the judgements, and what could
/// not be read. Nothing is written and nothing is judged.
///
/// It takes the read-only ports, so there is no port to write with; the surface turns the
/// [`Snapshot`] into its own report (the next steps are the domain's, asked of it).
pub async fn status<P: CorpusRead>(
    ports: &P,
    config: &Config,
) -> Result<Outcome<Snapshot>, AppError> {
    finish(
        Snapshot::read(ports, config.gate_policy())
            .await
            .map_err(Flow::from),
    )
}

#[cfg(test)]
mod tests;
