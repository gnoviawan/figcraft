use super::*;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no open document")]
    NoDocument,
    #[error("no such node {0}")]
    NoNode(NodeId),
    #[error("{0}")]
    Other(String),
    /// A command run in the background sent a request to a server and runs again with the answer
    /// ([`background`]); never returned to other callers.
    #[error("waiting for the server")]
    Waiting,
    /// A bug: the command panicked. The document was rolled back.
    #[error("internal error in `{cmd}`: {msg} (the document was left as it was before; please report this bug)")]
    Internal { cmd: String, msg: String },
}

impl From<figcraft_doc::DocError> for EngineError {
    fn from(e: figcraft_doc::DocError) -> Self {
        match e {
            figcraft_doc::DocError::NoNode(id) => EngineError::NoNode(id),
            other => EngineError::Other(other.to_string()),
        }
    }
}

impl EngineError {
    /// This error from a batch step, its message starting with `step`, the step's label
    /// ([`BatchScope::label`](crate::BatchScope::label)). The kind stays when it has a message. An
    /// unknown command, a missing layer or no document has none, so it becomes [`Other`](Self::Other)
    /// with the label in front, and the step is still named. `Waiting` stays as it is: the
    /// background runner reruns the batch on it.
    pub(crate) fn at_step(self, step: &str) -> Self {
        match self {
            EngineError::BadParams { cmd, msg } => EngineError::BadParams { cmd, msg: format!("{step}: {msg}") },
            EngineError::Other(msg) => EngineError::Other(format!("{step}: {msg}")),
            EngineError::Disabled(cmd, why) => EngineError::Disabled(cmd, format!("{step}: {why}")),
            EngineError::Internal { cmd, msg } => EngineError::Internal { cmd, msg: format!("{step}: {msg}") },
            e @ (EngineError::UnknownCommand(_) | EngineError::NoNode(_) | EngineError::NoDocument) => EngineError::Other(format!("{step}: {e}")),
            e @ EngineError::Waiting => e,
        }
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

pub fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_string(), msg: msg.into() }
}
