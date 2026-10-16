use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Configuration,
    InvalidTarget,
    UnsupportedXdmVersion,
    CorruptXdmState,
    MissingSegment,
    FfmpegUnavailable,
    FfmpegExecution,
    OutputConflict,
    Filesystem,
    Internal,
}

impl ErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Configuration => "Configuration",
            Self::InvalidTarget => "Nom cible invalide",
            Self::UnsupportedXdmVersion => "Version XDM non prise en charge",
            Self::CorruptXdmState => "État XDM illisible",
            Self::MissingSegment => "Segments manquants ou incomplets",
            Self::FfmpegUnavailable => "FFmpeg indisponible",
            Self::FfmpegExecution => "Échec FFmpeg",
            Self::OutputConflict => "Fichier cible déjà présent",
            Self::Filesystem => "Erreur de fichier",
            Self::Internal => "Erreur interne",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
    pub hint: Option<String>,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), hint: None }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} : {}", self.kind.label(), self.message)
    }
}

impl std::error::Error for AppError {}
