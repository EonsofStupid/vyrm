//! `RRD storage coordinator`: coordination between the canonical engine and immutable bytes.
//!
//! The engine remains the transaction coordinator. Objects are staged first;
//! their references, audit envelope, and projection work become visible in one
//! engine transaction. Failed commits leave explicit orphans for later GC.

use crate::{Engine, Error, ImmutableObjectStore, Result};
use rrd_core::{
    DataTransaction, ObjectReference, RuntimeCommitOutcome, RuntimeMutation, RuntimeRef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataRuntimeStep {
    BeforeObjectPut,
    AfterObjectPut,
    BeforeObjectVerification,
    AfterObjectVerification,
    BeforeCommit,
    AfterCommit,
}

pub struct DataRuntime<E, O> {
    engine: E,
    objects: O,
}

/// A non-owning view over the same data transaction coordinator.
///
/// Composition roots such as RRD own their engine and object tier for the
/// lifetime of the service. This view lets those roots use the identical
/// stage/verify/commit boundary without reopening either backend or creating
/// a second transaction authority.
pub struct DataRuntimeRef<'a, E, O> {
    engine: &'a E,
    objects: &'a O,
}

pub trait DataRuntimeAccess {
    type Engine: Engine;
    type Objects: ImmutableObjectStore;

    fn engine(&self) -> &Self::Engine;
    fn objects(&self) -> &Self::Objects;

    fn stage_object(
        &self,
        id: impl Into<String>,
        subject: Option<RuntimeRef>,
        media_type: impl Into<String>,
        bytes: &[u8],
    ) -> Result<ObjectReference> {
        let verified = self.objects().put(bytes)?;
        ObjectReference::for_bytes(id, subject, media_type, bytes, verified.receipt)
            .map_err(Error::from)
    }

    fn commit(&self, transaction: &DataTransaction) -> Result<RuntimeCommitOutcome> {
        transaction.validate()?;
        for mutation in &transaction.commit.mutations {
            let RuntimeMutation::Object { object } = mutation else {
                continue;
            };
            let verified = self.objects().verify(&object.sha256)?;
            if verified.length != object.length {
                return Err(Error::ObjectLengthMismatch {
                    expected: object.length,
                    actual: verified.length,
                });
            }
            if verified.receipt.backend != object.receipt.backend
                || verified.receipt.key != object.receipt.key
                || verified.receipt.version != object.receipt.version
            {
                return Err(Error::Object(
                    "committed object receipt differs from verified backend evidence".into(),
                ));
            }
        }
        let commit_id = transaction.commit.digest();
        match self.engine().runtime_commit_outcome(&commit_id)? {
            Some(outcome) => Ok(outcome),
            None => self.engine().commit_data_transaction(transaction),
        }
    }
}

impl<E: Engine, O: ImmutableObjectStore> DataRuntimeAccess for DataRuntime<E, O> {
    type Engine = E;
    type Objects = O;

    fn engine(&self) -> &Self::Engine {
        &self.engine
    }

    fn objects(&self) -> &Self::Objects {
        &self.objects
    }
}

impl<'a, E: Engine, O: ImmutableObjectStore> DataRuntimeRef<'a, E, O> {
    pub const fn new(engine: &'a E, objects: &'a O) -> Self {
        Self { engine, objects }
    }
}

impl<E: Engine, O: ImmutableObjectStore> DataRuntimeAccess for DataRuntimeRef<'_, E, O> {
    type Engine = E;
    type Objects = O;

    fn engine(&self) -> &Self::Engine {
        self.engine
    }

    fn objects(&self) -> &Self::Objects {
        self.objects
    }
}

impl<E: Engine, O: ImmutableObjectStore> DataRuntime<E, O> {
    pub fn new(engine: E, objects: O) -> Self {
        Self { engine, objects }
    }

    pub fn engine(&self) -> &E {
        &self.engine
    }

    pub fn objects(&self) -> &O {
        &self.objects
    }

    pub fn into_parts(self) -> (E, O) {
        (self.engine, self.objects)
    }

    pub fn stage_object(
        &self,
        id: impl Into<String>,
        subject: Option<RuntimeRef>,
        media_type: impl Into<String>,
        bytes: &[u8],
    ) -> Result<ObjectReference> {
        self.stage_object_with_hook(id, subject, media_type, bytes, |_| Ok(()))
    }

    pub fn stage_object_with_hook(
        &self,
        id: impl Into<String>,
        subject: Option<RuntimeRef>,
        media_type: impl Into<String>,
        bytes: &[u8],
        mut hook: impl FnMut(DataRuntimeStep) -> Result<()>,
    ) -> Result<ObjectReference> {
        hook(DataRuntimeStep::BeforeObjectPut)?;
        let verified = self.objects.put(bytes)?;
        hook(DataRuntimeStep::AfterObjectPut)?;
        ObjectReference::for_bytes(id, subject, media_type, bytes, verified.receipt)
            .map_err(Error::from)
    }

    pub fn commit(&self, transaction: &DataTransaction) -> Result<RuntimeCommitOutcome> {
        self.commit_with_hook(transaction, |_| Ok(()))
    }

    pub fn commit_with_hook(
        &self,
        transaction: &DataTransaction,
        mut hook: impl FnMut(DataRuntimeStep) -> Result<()>,
    ) -> Result<RuntimeCommitOutcome> {
        transaction.validate()?;
        hook(DataRuntimeStep::BeforeObjectVerification)?;
        for mutation in &transaction.commit.mutations {
            let RuntimeMutation::Object { object } = mutation else {
                continue;
            };
            let verified = self.objects.verify(&object.sha256)?;
            if verified.length != object.length {
                return Err(Error::ObjectLengthMismatch {
                    expected: object.length,
                    actual: verified.length,
                });
            }
            if verified.receipt.backend != object.receipt.backend
                || verified.receipt.key != object.receipt.key
                || verified.receipt.version != object.receipt.version
            {
                return Err(Error::Object(
                    "committed object receipt differs from verified backend evidence".into(),
                ));
            }
        }
        hook(DataRuntimeStep::AfterObjectVerification)?;
        hook(DataRuntimeStep::BeforeCommit)?;
        let commit_id = transaction.commit.digest();
        let outcome = match self.engine.runtime_commit_outcome(&commit_id)? {
            Some(outcome) => outcome,
            None => self.engine.commit_data_transaction(transaction)?,
        };
        hook(DataRuntimeStep::AfterCommit)?;
        Ok(outcome)
    }
}
