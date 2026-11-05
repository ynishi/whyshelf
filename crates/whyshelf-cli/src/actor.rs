//! The task that owns the workspace for the tool server.
//!
//! A tool server answers requests that may arrive together, but a workspace
//! writes one change at a time: each write loads the tips of a stream, decides,
//! records and notifies, and two of them interleaved would fork the stream. The
//! use cases already say so in their types (a write takes `&mut Workspace`). The
//! server therefore does not share the workspace behind a lock; it gives it to
//! one task, the [`WorkspaceActor`], and talks to the task through a bounded
//! queue. Each request carries a one-shot channel for its answer, and the task
//! runs the requests one after the other, so the queue is the order of the writes.
//!
//! The task ends when every [`ActorHandle`] is dropped, and then gives the
//! workspace back, so that the caller can close what the adapters opened.

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use whyshelf_core::app::Workspace;
use whyshelf_core::app::ports::Ports;
use whyshelf_core::app::view::ContextLine;

use crate::command::{Command, Done, execute, warning};
use crate::error::CliError;

/// How many requests may wait for the task before senders wait in turn.
const QUEUE: usize = 16;

/// What the task answers: how the command went, and where the tool was pointed
/// after it, a failed command included.
#[derive(Debug)]
pub struct Answer {
    /// The outcome and its warnings, or why the command failed.
    pub result: Result<Done, CliError>,
    /// The context after the command ran; if it could not be read, the warning
    /// line that says why (the failure is carried, not turned into "no context").
    pub context: Result<ContextLine, String>,
}

struct Request {
    command: Command,
    reply: oneshot::Sender<Answer>,
}

/// The way to the task: cheap to clone, one per concurrent caller.
#[derive(Debug, Clone)]
pub struct ActorHandle {
    queue: mpsc::Sender<Request>,
}

impl ActorHandle {
    /// Send a command and wait for its answer.
    pub async fn call(&self, command: Command) -> Result<Answer, CliError> {
        let (reply, answer) = oneshot::channel();
        self.queue
            .send(Request { command, reply })
            .await
            .map_err(|_| CliError::ActorStopped)?;
        answer.await.map_err(|_| CliError::ActorStopped)
    }
}

/// The task that runs commands on a workspace, one at a time.
#[derive(Debug)]
pub struct WorkspaceActor<P: Ports> {
    workspace: Workspace<P>,
    queue: mpsc::Receiver<Request>,
}

impl<P: Ports + 'static> WorkspaceActor<P> {
    /// Start the task. It runs until every handle is dropped, and returns the
    /// workspace then.
    pub fn spawn(workspace: Workspace<P>) -> (ActorHandle, JoinHandle<Workspace<P>>) {
        let (sender, queue) = mpsc::channel(QUEUE);
        let actor = Self { workspace, queue };
        (ActorHandle { queue: sender }, tokio::spawn(actor.run()))
    }

    async fn run(mut self) -> Workspace<P> {
        while let Some(Request { command, reply }) = self.queue.recv().await {
            let answer = self.answer(command).await;
            // The caller may have given up waiting; the command has run anyway.
            let _ = reply.send(answer);
        }
        self.workspace
    }

    async fn answer(&mut self, command: Command) -> Answer {
        let result = execute(&mut self.workspace, command).await;
        let context = match self.workspace.context().await {
            Ok(report) => Ok(report.line().clone()),
            Err(error) => Err(format!(
                "the context line could not be read: {}",
                warning(&error)
            )),
        };
        Answer { result, context }
    }
}

#[cfg(test)]
mod tests;
