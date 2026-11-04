use crate::tr;

#[derive(Clone, Debug)]
pub(crate) enum AppError {
    Message(String),
    ReadFile(String),
    OpenFile(String),
    Request(String),
    ReadResponse(String),
    Parse(String),
    Save(String),
    Load(String),
}

impl AppError {
    pub(crate) fn plain(err: impl ToString) -> Self {
        Self::Message(err.to_string())
    }

    pub(crate) fn read_file(err: impl ToString) -> Self {
        Self::ReadFile(err.to_string())
    }

    pub(crate) fn open_file(err: impl ToString) -> Self {
        Self::OpenFile(err.to_string())
    }

    pub(crate) fn request(err: impl ToString) -> Self {
        Self::Request(err.to_string())
    }

    pub(crate) fn read_response(err: impl ToString) -> Self {
        Self::ReadResponse(err.to_string())
    }

    pub(crate) fn parse(err: impl ToString) -> Self {
        Self::Parse(err.to_string())
    }

    pub(crate) fn save(err: impl ToString) -> Self {
        Self::Save(err.to_string())
    }

    pub(crate) fn load(err: impl ToString) -> Self {
        Self::Load(err.to_string())
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Message(err) => err.clone(),
            Self::ReadFile(err) => tr!("Could not read file: %{err}", err = err).to_string(),
            Self::OpenFile(err) => tr!("Could not open file: %{err}", err = err).to_string(),
            Self::Request(err) => tr!("Request failed: %{err}", err = err).to_string(),
            Self::ReadResponse(err) => {
                tr!("Could not read response: %{err}", err = err).to_string()
            }
            Self::Parse(err) => tr!("Parse failed: %{err}", err = err).to_string(),
            Self::Save(err) => tr!("Save failed: %{err}", err = err).to_string(),
            Self::Load(err) => tr!("Load failed: %{err}", err = err).to_string(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Message(_) => "message",
            Self::ReadFile(_) => "read file",
            Self::OpenFile(_) => "open file",
            Self::Request(_) => "request",
            Self::ReadResponse(_) => "read response",
            Self::Parse(_) => "parse",
            Self::Save(_) => "save",
            Self::Load(_) => "load",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::Message(err)
            | Self::ReadFile(err)
            | Self::OpenFile(err)
            | Self::Request(err)
            | Self::ReadResponse(err)
            | Self::Parse(err)
            | Self::Save(err)
            | Self::Load(err) => err,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if matches!(self, Self::Message(_)) {
            f.write_str(self.detail())
        } else {
            write!(f, "{} failed: {}", self.kind(), self.detail())
        }
    }
}

impl std::error::Error for AppError {}

/// A transient user-facing status message.
#[derive(Clone)]
pub(crate) enum Status {
    Success(String),
    Error(AppError),
}

impl Status {
    pub(crate) fn from_parts(ok: bool, text: impl Into<String>) -> Self {
        if ok {
            Self::success(text)
        } else {
            Self::Error(AppError::plain(text.into()))
        }
    }

    pub(crate) fn success(text: impl Into<String>) -> Self {
        Self::Success(text.into())
    }

    pub(crate) fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Success(text) => text.clone(),
            Self::Error(error) => error.message(),
        }
    }
}
