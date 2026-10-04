//! The error every typed operation returns. `toolportctl`, the `plus.*` handlers and the self-MCP
//! tools are adapters over the operations; each maps this to its own wire form.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    NotFound,
    Conflict,
    NotImplemented,
    Unhealthy,
    Failed(&'static str),
}

impl ErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::NotImplemented => "not_implemented",
            Self::Unhealthy => "unhealthy",
            Self::Failed(code) => code,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpError {
    pub kind: ErrorKind,
    pub message: String,
}

impl OpError {
    fn of(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Failed(code), message)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Usage, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Conflict, message)
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::NotImplemented, message)
    }

    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Unhealthy, message)
    }

    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}
