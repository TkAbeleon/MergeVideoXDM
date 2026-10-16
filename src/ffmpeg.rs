//! Interface isolée avec le binaire FFmpeg du système.
use std::ffi::OsString;
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

/// Segments d'une piste vidéo et d'une piste audio à assembler en une seule passe.
pub struct TrackPaths<'a> {
    pub video: &'a [PathBuf],
    pub audio: &'a [PathBuf],
}

/// Assemble une piste vidéo et une piste audio dans le fichier final avec **un seul**
/// processus FFmpeg : chaque piste est lue par son propre démultiplexeur `concat`, puis
/// les flux sont copiés tels quels dans le conteneur cible.
///
/// Comparé à « concat vidéo → concat audio → mux », cela évite d'écrire (puis de relire)
/// deux fichiers intermédiaires aussi gros que la vidéo téléchargée.
pub fn concat_mux<F>(
    tracks: &TrackPaths<'_>,
    work_dir: &Path,
    destination: &Path,
    extension: &str,
    expected_seconds: f64,
    progress_range: (f32, f32),
    on_progress: &mut F,
) -> Result<(), AppError>
where
    F: FnMut(ProgressUpdate),
{
    let video_manifest = work_dir.join("video.ffconcat");
    let audio_manifest = work_dir.join("audio.ffconcat");
    write_manifest(tracks.video, &video_manifest)?;
    write_manifest(tracks.audio, &audio_manifest)?;

    let mut command = Command::new("ffmpeg");
    command.args(concat_mux_args(
        &video_manifest,
        &audio_manifest,
        destination,
        extension,
    ));

    run_with_progress(
        command,
        expected_seconds,
        "Assemblage audio/vidéo",
        progress_range,
        on_progress,
    )?;
    ensure_non_empty(destination)
}

fn concat_mux_args(
    video_manifest: &Path,
    audio_manifest: &Path,
    destination: &Path,
    extension: &str,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
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
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.push(video_manifest.into());
    args.extend(
        ["-f", "concat", "-safe", "0", "-i"]
            .iter()
            .map(OsString::from),
    );
    args.push(audio_manifest.into());
    args.extend(
        ["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]
            .iter()
            .map(OsString::from),
    );
    if matches!(extension, "mp4" | "m4v") {
        args.extend(["-movflags", "+faststart"].iter().map(OsString::from));
    }
    args.push(destination.into());
    args
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
        AppError::new(
            ErrorKind::Internal,
            "Le flux de progression FFmpeg est indisponible.",
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        AppError::new(
            ErrorKind::Internal,
            "Le flux d'erreurs FFmpeg est indisponible.",
        )
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
        return Err(AppError::new(
            ErrorKind::FfmpegExecution,
            "FFmpeg a produit un fichier vide.",
        ));
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
        AppError::new(
            ErrorKind::Filesystem,
            format!("Écriture du manifeste impossible : {error}"),
        )
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
            AppError::new(
                ErrorKind::Filesystem,
                format!("Écriture du manifeste impossible : {error}"),
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{ProgressUpdate, TrackPaths, concat_mux, concat_mux_args, parse_clock};

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

    #[test]
    fn concat_mux_args_map_one_video_and_one_audio_input() {
        let args: Vec<String> = concat_mux_args(
            Path::new("/w/video.ffconcat"),
            Path::new("/w/audio.ffconcat"),
            Path::new("/o/out.mp4"),
            "mp4",
        )
        .into_iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();

        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        assert!(args.windows(2).any(|w| w == ["-map", "0:v:0"]));
        assert!(args.windows(2).any(|w| w == ["-map", "1:a:0"]));
        assert!(args.windows(2).any(|w| w == ["-movflags", "+faststart"]));
        assert_eq!(args.last().map(String::as_str), Some("/o/out.mp4"));
    }

    #[test]
    fn concat_mux_args_skip_faststart_for_non_mp4() {
        let args = concat_mux_args(Path::new("v"), Path::new("a"), Path::new("out.mkv"), "mkv");
        assert!(!args.iter().any(|a| a == "+faststart"));
    }

    /// Génère de vrais segments MPEG-TS (vidéo seule, audio seul) comme le fait XDM pour
    /// du HLS à pistes séparées. Retourne `None` si FFmpeg n'est pas installé.
    fn synthetic_segments(
        dir: &Path,
        name: &str,
        input: &[&str],
        codec: &[&str],
    ) -> Option<Vec<PathBuf>> {
        let pattern = dir.join(format!("{name}-%03d.ts"));
        let status = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y"])
            .args(input)
            .args(["-t", "6"])
            .args(codec)
            .args([
                "-f",
                "segment",
                "-segment_time",
                "2",
                "-segment_format",
                "mpegts",
            ])
            .arg(&pattern)
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|file| file.to_str())
                    .is_some_and(|file| file.starts_with(name) && file.ends_with(".ts"))
            })
            .collect();
        files.sort();
        Some(files)
    }

    #[test]
    fn concat_mux_produces_a_playable_file_with_both_tracks() {
        let dir = std::env::temp_dir().join(format!("mvxdm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let video = synthetic_segments(
            &dir,
            "v",
            &["-f", "lavfi", "-i", "testsrc=size=160x120:rate=25"],
            &["-c:v", "libx264", "-g", "50", "-pix_fmt", "yuv420p"],
        );
        let audio = synthetic_segments(
            &dir,
            "a",
            &["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100"],
            &["-c:a", "aac"],
        );
        let (Some(video), Some(audio)) = (video, audio) else {
            eprintln!("FFmpeg indisponible (ou sans libx264) : test ignoré");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        assert!(video.len() >= 2 && audio.len() >= 2);

        let output = dir.join("out.mp4");
        let mut last_fraction = 0.0_f32;
        concat_mux(
            &TrackPaths {
                video: &video,
                audio: &audio,
            },
            &dir,
            &output,
            "mp4",
            6.0,
            (0.03, 0.98),
            &mut |update: ProgressUpdate| last_fraction = update.fraction,
        )
        .expect("l'assemblage en une passe doit réussir");

        assert!((last_fraction - 0.98).abs() < f32::EPSILON);
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output();
        if let Ok(probe) = probe {
            let streams = String::from_utf8_lossy(&probe.stdout);
            assert!(
                streams.contains("video") && streams.contains("audio"),
                "{streams}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
