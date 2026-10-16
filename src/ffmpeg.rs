//! Interface isolée avec le binaire FFmpeg du système.
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use crate::domain::ProgressUpdate;
use crate::errors::{AppError, ErrorKind};

pub fn info() -> Result<String, AppError> {
    let output = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map_err(|_| {
            AppError::new(
                ErrorKind::FfmpegUnavailable,
                "La commande ffmpeg n'est pas disponible dans PATH.",
            )
            .with_hint("Installe FFmpeg avec le gestionnaire de paquets de ta distribution.")
        })?;

    if !output.status.success() {
        return Err(AppError::new(
            ErrorKind::FfmpegUnavailable,
            "La commande ffmpeg existe mais ne répond pas correctement.",
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("FFmpeg détecté")
        .to_string())
}

pub fn concat<F>(
    paths: &[PathBuf],
    manifest_path: &Path,
    destination: &Path,
    expected_seconds: f64,
    phase: &str,
    progress_range: (f32, f32),
    on_progress: &mut F,
) -> Result<(), AppError>
where
    F: FnMut(ProgressUpdate),
{
    write_manifest(paths, manifest_path)?;

    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "warning",
            "-y",
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(manifest_path)
        .args(["-map", "0", "-c", "copy"])
        .arg(destination);

    run_with_progress(
        command,
        expected_seconds,
        phase,
        progress_range,
        on_progress,
    )?;
    ensure_non_empty(destination)
}

pub fn mux<F>(
    video: &Path,
    audio: &Path,
    destination: &Path,
    extension: &str,
    expected_seconds: f64,
    progress_range: (f32, f32),
    on_progress: &mut F,
) -> Result<(), AppError>
where
    F: FnMut(ProgressUpdate),
{
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "warning",
            "-y",
            "-progress",
            "pipe:1",
            "-nostats",
            "-i",
        ])
        .arg(video)
        .arg("-i")
        .arg(audio)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]);

    if matches!(extension, "mp4" | "m4v") {
        command.args(["-movflags", "+faststart"]);
    }

    command.arg(destination);
    run_with_progress(
        command,
        expected_seconds,
        "Synchronisation audio/vidéo",
        progress_range,
        on_progress,
    )?;
    ensure_non_empty(destination)
}

fn run_with_progress<F>(
    mut command: Command,
    expected_seconds: f64,
    phase: &str,
    (range_start, range_end): (f32, f32),
    on_progress: &mut F,
) -> Result<(), AppError>
where
    F: FnMut(ProgressUpdate),
{
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        AppError::new(
            ErrorKind::FfmpegUnavailable,
            format!("Impossible de lancer FFmpeg : {error}"),
        )
        .with_hint("Vérifie l'installation de FFmpeg et la variable PATH.")
    })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        AppError::new(ErrorKind::Internal, "Le flux de progression FFmpeg est indisponible.")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        AppError::new(ErrorKind::Internal, "Le flux d'erreurs FFmpeg est indisponible.")
    })?;

    // Lire stderr en parallèle évite de bloquer FFmpeg si sa sortie de diagnostic se remplit.
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = stderr;
        let _ = reader.read_to_end(&mut bytes);
        bytes
    });

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut out_seconds = 0.0_f64;
    let mut last_emitted = range_start;
    let mut stream_error = None;

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let value = line.trim();
                if let Some(raw) = value.strip_prefix("out_time_ms=") {
                    // FFmpeg garde ce nom historique, mais la valeur est exprimée en microsecondes.
                    if let Ok(micros) = raw.parse::<f64>() {
                        out_seconds = (micros / 1_000_000.0).max(0.0);
                    }
                } else if let Some(raw) = value.strip_prefix("out_time_us=") {
                    if let Ok(micros) = raw.parse::<f64>() {
                        out_seconds = (micros / 1_000_000.0).max(0.0);
                    }
                } else if let Some(raw) = value.strip_prefix("out_time=") {
                    if let Some(seconds) = parse_clock(raw) {
                        out_seconds = seconds;
                    }
                } else if value == "progress=continue" || value == "progress=end" {
                    let is_end = value == "progress=end";
                    let fraction = if is_end {
                        range_end
                    } else if expected_seconds.is_finite() && expected_seconds > 0.0 {
                        let ratio = (out_seconds / expected_seconds).clamp(0.0, 1.0) as f32;
                        range_start + (range_end - range_start) * ratio
                    } else {
                        range_start
                    };

                    if is_end || fraction - last_emitted >= 0.01 {
                        on_progress(ProgressUpdate {
                            fraction,
                            stage: phase.to_string(),
                            message: format!("{phase} : {:.0} %", fraction * 100.0),
                        });
                        last_emitted = fraction;
                    }
                }
            }
            Err(error) => {
                stream_error = Some(error);
                break;
            }
        }
    }

    let status = child.wait().map_err(|error| {
        AppError::new(
            ErrorKind::FfmpegExecution,
            format!("Impossible d'attendre la fin de FFmpeg : {error}"),
        )
    })?;
    let stderr = stderr_reader.join().unwrap_or_default();

    if let Some(error) = stream_error {
        return Err(AppError::new(
            ErrorKind::FfmpegExecution,
            format!("Lecture de la progression FFmpeg impossible : {error}"),
        ));
    }

    if !status.success() {
        return Err(AppError::new(
            ErrorKind::FfmpegExecution,
            format!(
                "FFmpeg a terminé avec le code {}.\n{}",
                status.code().map_or_else(|| "inconnu".into(), |code| code.to_string()),
                last_lines(&stderr)
            ),
        )
        .with_hint(
            "Consulte les détails de la tâche. Si les codecs refusent le conteneur choisi, essaie .mkv ou .ts ; aucun réencodage automatique n'est lancé.",
        ));
    }

    on_progress(ProgressUpdate {
        fraction: range_end,
        stage: phase.to_string(),
        message: format!("{phase} : terminé"),
    });
    Ok(())
}

fn parse_clock(value: &str) -> Option<f64> {
    let mut parts = value.split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() || hours < 0.0 || minutes < 0.0 || seconds < 0.0 {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn last_lines(stderr: &[u8]) -> String {
    let value = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = value.lines().rev().take(8).collect();
    if lines.is_empty() {
        "FFmpeg n'a fourni aucun détail supplémentaire.".into()
    } else {
        lines.into_iter().rev().collect::<Vec<_>>().join("\n")
    }
}

fn ensure_non_empty(path: &Path) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(|error| {
        AppError::new(
            ErrorKind::Filesystem,
            format!("FFmpeg n'a pas créé la sortie attendue : {error}"),
        )
    })?;
    if metadata.len() == 0 {
        return Err(AppError::new(ErrorKind::FfmpegExecution, "FFmpeg a produit un fichier vide."));
    }
    Ok(())
}

fn write_manifest(paths: &[PathBuf], manifest_path: &Path) -> Result<(), AppError> {
    let mut file = fs::File::create(manifest_path).map_err(|error| {
        AppError::new(
            ErrorKind::Filesystem,
            format!("Création de la liste des segments impossible : {error}"),
        )
    })?;
    writeln!(file, "ffconcat version 1.0").map_err(|error| {
        AppError::new(ErrorKind::Filesystem, format!("Écriture du manifeste impossible : {error}"))
    })?;

    for path in paths {
        let absolute = path.canonicalize().map_err(|error| {
            AppError::new(
                ErrorKind::Filesystem,
                format!("Segment inaccessible {} : {error}", path.display()),
            )
        })?;
        let value = absolute.to_string_lossy().replace('\\', "/");
        writeln!(file, "file '{}'", value.replace('\'', "'\\''")).map_err(|error| {
            AppError::new(ErrorKind::Filesystem, format!("Écriture du manifeste impossible : {error}"))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_clock, ProgressUpdate};

    #[test]
    fn parses_ffmpeg_clock_format() {
        assert_eq!(parse_clock("00:00:12.500"), Some(12.5));
        assert_eq!(parse_clock("01:02:03.250"), Some(3723.25));
        assert_eq!(parse_clock("invalid"), None);
    }

    #[test]
    fn progress_updates_are_values_between_zero_and_one() {
        let update = ProgressUpdate {
            fraction: 0.75,
            stage: "Concaténation".into(),
            message: "75 %".into(),
        };
        assert!((0.0..=1.0).contains(&update.fraction));
    }
}
