//! Adaptateurs pour les différents formats de métadonnées XDM.
//!
//! Pour ajouter une version, créer un module (par exemple v830), implémenter
//! XdmStateAdapter, puis l'ajouter au registre ci-dessous. Le moteur de fusion
//! ne doit pas connaître les détails de sérialisation propres à une version.
mod v829;

use std::fs;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

use crate::errors::{AppError, ErrorKind};

#[derive(Debug, Clone)]
pub struct XdmSegment {
    pub id: String,
    pub url: String,
    pub stream_index: i32,
    pub status: i32,
    pub duration_seconds: f64,
}

impl XdmSegment {
    pub fn is_finished(&self) -> bool {
        self.status == 4
    }

    pub fn status_label(&self) -> &'static str {
        match self.status {
            0 => "Ready",
            1 => "FailedFatal",
            2 => "FailedTransient",
            3 => "InProgress",
            4 => "Finished",
            _ => "Inconnu",
        }
    }

    pub fn file_path(&self, folder: &Path) -> PathBuf {
        let prefix = if self.stream_index == 0 { "1_" } else { "2_" };
        folder.join(format!(
            "{prefix}{}{}",
            self.id,
            safe_url_filename(&self.url)
        ))
    }
}

#[derive(Debug, Clone)]
pub struct XdmState {
    pub version: String,
    pub state_file: PathBuf,
    /// L'ordre de ce vecteur est l'ordre d'assemblage ; ne jamais le trier par nom.
    pub segments: Vec<XdmSegment>,
}

pub trait XdmStateAdapter: Sync {
    fn version(&self) -> &'static str;
    fn recognizes(&self, filename: &str, bytes: &[u8]) -> bool;
    fn parse(&self, path: &Path, bytes: &[u8]) -> Result<XdmState, AppError>;
}

pub fn supported_versions() -> Vec<&'static str> {
    adapters().iter().map(|adapter| adapter.version()).collect()
}

fn adapters() -> Vec<&'static dyn XdmStateAdapter> {
    static XDM_829: v829::Adapter = v829::Adapter;
    vec![&XDM_829]
}

pub fn read_state(folder: &Path) -> Result<XdmState, AppError> {
    if !folder.is_dir() {
        return Err(AppError::new(
            ErrorKind::Filesystem,
            format!("Le dossier source n'existe pas : {}", folder.display()),
        ));
    }

    let candidates = ["chunks.db.1", "chunks.db.2"];
    let mut found_any = false;
    let mut diagnostics = Vec::new();
    let mut parse_errors = Vec::new();

    for filename in candidates {
        let path = folder.join(filename);
        if !path.is_file() {
            continue;
        }
        found_any = true;
        let bytes = fs::read(&path).map_err(|error| {
            AppError::new(
                ErrorKind::Filesystem,
                format!("Lecture impossible de {} : {error}", path.display()),
            )
        })?;

        let mut recognized = false;
        for adapter in adapters() {
            if adapter.recognizes(filename, &bytes) {
                recognized = true;
                match adapter.parse(&path, &bytes) {
                    Ok(state) => return Ok(state),
                    Err(error) => parse_errors.push(format!(
                        "{filename} reconnu comme XDM {} mais invalide : {}",
                        adapter.version(),
                        error.message
                    )),
                }
            }
        }
        if !recognized {
            diagnostics.push(format!(
                "{filename}: aucun adaptateur connu ne reconnaît ce format"
            ));
        }
    }

    if !found_any {
        return Err(AppError::new(
            ErrorKind::CorruptXdmState,
            format!(
                "Aucun fichier chunks.db.1 ou chunks.db.2 trouvé dans {}",
                folder.display()
            ),
        )
        .with_hint(
            "Sélectionne le dossier temporaire d'une tâche XDM, pas le dossier parent temp.",
        ));
    }

    if !parse_errors.is_empty() {
        return Err(AppError::new(
            ErrorKind::CorruptXdmState,
            parse_errors.join("; "),
        )
        .with_hint(
            "Ferme XDM proprement, sauvegarde le dossier de tâche, puis réessaie. Un autre fichier d'état XDM valide a été recherché avant l'échec.",
        ));
    }

    Err(AppError::new(
        ErrorKind::UnsupportedXdmVersion,
        diagnostics.join("; "),
    )
    .with_hint(format!(
        "Versions actuellement prises en charge : {}. Un nouvel adaptateur peut être ajouté dans src/xdm/.",
        supported_versions().join(", ")
    )))
}

fn safe_url_filename(value: &str) -> String {
    let raw_name = Url::parse(value)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|parts| parts.last().map(str::to_owned))
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "segment.ts".into());
    let decoded = percent_decode_str(&raw_name).decode_utf8_lossy();
    let safe: String = decoded
        .chars()
        .map(|character| {
            if character == '/' || character == '\\' || character == '\0' {
                '_'
            } else {
                character
            }
        })
        .collect();
    if safe.is_empty() || safe == "." || safe == ".." {
        "segment.ts".into()
    } else {
        safe
    }
}
