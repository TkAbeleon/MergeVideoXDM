//! Orchestration indépendante de la version de XDM et des détails de FFmpeg.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::Builder as TempBuilder;

use crate::domain::{MergeReport, MergeRequest, ProgressUpdate};
use crate::errors::{AppError, ErrorKind};
use crate::{ffmpeg, xdm::{self, XdmSegment}};

pub fn merge<F>(request: MergeRequest, mut on_progress: F) -> Result<MergeReport, AppError>
where
    F: FnMut(ProgressUpdate),
{
    let (target_name, extension) = validate_target_name(&request.target_name)?;
    let ffmpeg_version = ffmpeg::info()?;
    emit(
        &mut on_progress,
        0.01,
        "Analyse XDM",
        format!("Moteur détecté : {ffmpeg_version}"),
    );

    let state = xdm::read_state(&request.source_dir)?;
    emit(
        &mut on_progress,
        0.02,
        format!("Lecture XDM {}", state.version),
        format!("{} segments référencés dans {}", state.segments.len(), state.state_file.display()),
    );

    if state.segments.is_empty() {
        return Err(AppError::new(ErrorKind::CorruptXdmState, "La base XDM ne contient aucun segment."));
    }

    fs::create_dir_all(&request.output_dir).map_err(|error| {
        AppError::new(
            ErrorKind::Filesystem,
            format!("Création du dossier de sortie impossible : {error}"),
        )
    })?;

    let output_dir = request.output_dir.canonicalize().map_err(|error| {
        AppError::new(
            ErrorKind::Filesystem,
            format!("Accès au dossier de sortie impossible : {error}"),
        )
    })?;
    let final_target = output_dir.join(&target_name);
    if final_target.exists() {
        return Err(AppError::new(
            ErrorKind::OutputConflict,
            format!("Le fichier cible existe déjà : {}", final_target.display()),
        )
        .with_hint("Choisis un autre nom ou déplace le résultat existant. Les fichiers ne sont jamais écrasés."));
    }

    let mut streams: BTreeMap<i32, Vec<(&XdmSegment, PathBuf)>> = BTreeMap::new();
    let mut unfinished = Vec::new();
    let mut missing = Vec::new();
    let mut readable_count = 0usize;

    for segment in &state.segments {
        let path = segment.file_path(&request.source_dir);
        if !segment.is_finished() {
            unfinished.push(format!(
                "{} ({})",
                path.file_name().unwrap_or_default().to_string_lossy(),
                segment.status_label()
            ));
        }

        if !path.is_file() {
            missing.push(format!("Segment absent : {}", path.file_name().unwrap_or_default().to_string_lossy()));
            continue;
        }

        let metadata = fs::metadata(&path).map_err(|error| {
            AppError::new(
                ErrorKind::Filesystem,
                format!("Lecture impossible de {} : {error}", path.display()),
            )
        })?;
        if metadata.len() == 0 {
            missing.push(format!("Segment vide : {}", path.file_name().unwrap_or_default().to_string_lossy()));
            continue;
        }
        if !segment.is_finished() && !request.include_incomplete {
            continue;
        }
        readable_count += 1;
        streams.entry(segment.stream_index).or_default().push((segment, path));
    }

    let partial = !unfinished.is_empty() || !missing.is_empty();
    if partial && !request.include_incomplete {
        let mut details = Vec::new();
        details.extend(unfinished.iter().take(5).cloned());
        details.extend(missing.iter().take(5).cloned());
        let shown = details.join("\n");
        return Err(AppError::new(
            ErrorKind::MissingSegment,
            format!(
                "Mode sûr : {} segment(s) non terminés et {} problème(s) de fichiers. Aucun assemblage n'a été lancé.\n{}",
                unfinished.len(),
                missing.len(),
                shown
            ),
        )
        .with_hint("Sauvegarde le dossier XDM. Le mode de récupération partielle tente d'assembler les segments présents, mais le résultat peut être tronqué."));
    }

    if streams.is_empty() {
        return Err(AppError::new(
            ErrorKind::MissingSegment,
            "Aucun segment lisible n'est disponible.",
        ));
    }

    let stream_ids: Vec<i32> = streams.keys().copied().collect();
    if stream_ids != [0] && stream_ids != [0, 1] {
        return Err(AppError::new(
            ErrorKind::UnsupportedXdmVersion,
            format!("Disposition des pistes XDM non prise en charge : {stream_ids:?}."),
        ));
    }

    let output_temp_dir = TempBuilder::new()
        .prefix(".mergevideoxdm-work-")
        .tempdir_in(&output_dir)
        .map_err(|error| {
            AppError::new(ErrorKind::Filesystem, format!("Création du dossier temporaire impossible : {error}"))
        })?;

    let final_temp = TempBuilder::new()
        .prefix(".mergevideoxdm-final-")
        .suffix(&format!(".{extension}"))
        .tempfile_in(&output_dir)
        .map_err(|error| {
            AppError::new(ErrorKind::Filesystem, format!("Préparation du fichier cible impossible : {error}"))
        })?;

    if stream_ids == [0] {
        let track = &streams[&0];
        let paths: Vec<PathBuf> = track.iter().map(|(_, path)| path.clone()).collect();
        let expected = total_duration(track.iter().map(|(segment, _)| segment.duration_seconds));
        emit(
            &mut on_progress,
            0.03,
            "Préparation de FFmpeg",
            format!("{readable_count} segment(s) vidéo à assembler."),
        );
        ffmpeg::concat(
            &paths,
            &output_temp_dir.path().join("segments.ffconcat"),
            final_temp.path(),
            expected,
            "Assemblage des segments",
            (0.03, 0.98),
            &mut on_progress,
        )?;
    } else {
        let video = &streams[&0];
        let audio = &streams[&1];
        let video_paths: Vec<PathBuf> = video.iter().map(|(_, path)| path.clone()).collect();
        let audio_paths: Vec<PathBuf> = audio.iter().map(|(_, path)| path.clone()).collect();
        let video_path = output_temp_dir.path().join(format!(
            "video.{}",
            segment_extension(video.iter().map(|(segment, _)| *segment))
        ));
        let audio_path = output_temp_dir.path().join(format!(
            "audio.{}",
            segment_extension(audio.iter().map(|(segment, _)| *segment))
        ));

        ffmpeg::concat(
            &video_paths,
            &output_temp_dir.path().join("video.ffconcat"),
            &video_path,
            total_duration(video.iter().map(|(segment, _)| segment.duration_seconds)),
            "Assemblage de la piste vidéo",
            (0.03, 0.43),
            &mut on_progress,
        )?;
        ffmpeg::concat(
            &audio_paths,
            &output_temp_dir.path().join("audio.ffconcat"),
            &audio_path,
            total_duration(audio.iter().map(|(segment, _)| segment.duration_seconds)),
            "Assemblage de la piste audio",
            (0.43, 0.72),
            &mut on_progress,
        )?;
        ffmpeg::mux(
            &video_path,
            &audio_path,
            final_temp.path(),
            &extension,
            total_duration(video.iter().map(|(segment, _)| segment.duration_seconds)),
            (0.72, 0.98),
            &mut on_progress,
        )?;
    }

    // Re-vérification juste avant le renommage : ne jamais écraser une cible concurrente.
    if final_target.exists() {
        return Err(AppError::new(
            ErrorKind::OutputConflict,
            format!("Un autre processus a créé la cible : {}", final_target.display()),
        ));
    }
    final_temp.persist_noclobber(&final_target).map_err(|error| {
        AppError::new(
            ErrorKind::Filesystem,
            format!("Impossible d'enregistrer le résultat sans écrasement : {}", error.error),
        )
    })?;

    let mut detail = format!(
        "{readable_count} segment(s) assemblé(s) avec FFmpeg, sans réencodage."
    );
    if partial {
        detail.push_str(" Récupération partielle : certains segments sont incomplets ou absents.");
    }

    emit(
        &mut on_progress,
        1.0,
        "Terminé",
        format!("Fichier créé : {}", final_target.display()),
    );

    Ok(MergeReport {
        output: final_target,
        segment_count: readable_count,
        partial,
        xdm_version: state.version,
        detail,
    })
}

fn validate_target_name(name: &str) -> Result<(String, String), AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(ErrorKind::InvalidTarget, "Le nom cible est vide."));
    }
    if trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(AppError::new(
            ErrorKind::InvalidTarget,
            "Le nom cible doit être un nom de fichier, pas un chemin.",
        ));
    }

    let extension = Path::new(trimmed)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["mp4", "mkv", "ts", "mov", "m4v"].contains(&extension.as_str()) {
        return Err(AppError::new(
            ErrorKind::InvalidTarget,
            "Extension non prise en charge. Utilise .mp4, .mkv, .ts, .mov ou .m4v.",
        ));
    }
    Ok((trimmed.to_string(), extension))
}

fn total_duration(values: impl Iterator<Item = f64>) -> f64 {
    values.filter(|value| value.is_finite() && *value > 0.0).sum()
}

fn segment_extension<'a>(segments: impl Iterator<Item = &'a XdmSegment>) -> String {
    for segment in segments {
        if let Ok(url) = url::Url::parse(&segment.url) {
            if let Some(extension) = url
                .path_segments()
                .and_then(|parts| parts.last())
                .and_then(|name| Path::new(name).extension())
                .and_then(|extension| extension.to_str())
            {
                if !extension.is_empty()
                    && extension.len() <= 8
                    && extension.chars().all(|character| character.is_ascii_alphanumeric())
                {
                    return extension.to_ascii_lowercase();
                }
            }
        }
    }
    "ts".into()
}

fn emit<F>(callback: &mut F, fraction: f32, stage: impl Into<String>, message: impl Into<String>)
where
    F: FnMut(ProgressUpdate),
{
    callback(ProgressUpdate {
        fraction,
        stage: stage.into(),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::validate_target_name;

    #[test]
    fn validates_target_name_and_container_extension() {
        assert!(validate_target_name("ma-video.mp4").is_ok());
        assert!(validate_target_name("ma-video.mkv").is_ok());
        assert!(validate_target_name("../outside.mp4").is_err());
        assert!(validate_target_name("video.exe").is_err());
        assert!(validate_target_name(" ").is_err());
    }
}
