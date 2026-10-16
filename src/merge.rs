//! Lecture du format binaire XDM 8.0.29 et assemblage sans réencodage via FFmpeg.
use percent_encoding::percent_decode_str;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::Builder as TempBuilder;
use url::Url;

const FOOTER: &[u8; 4] = b"END.";
const MAX_STRING_SIZE: usize = 64 * 1024 * 1024;
const MAX_CHUNKS: usize = 2_000_000;

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
    pub target_name: String,
    pub include_incomplete: bool,
}

#[derive(Debug, Clone)]
pub struct MergeReport {
    pub output: PathBuf,
    pub segment_count: usize,
    pub partial: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
struct Chunk {
    id: String,
    stream_index: i32,
    status: i32,
    url: String,
}

impl Chunk {
    fn is_finished(&self) -> bool {
        self.status == 4
    }

    fn status_label(&self) -> &'static str {
        match self.status {
            0 => "Ready",
            1 => "FailedFatal",
            2 => "FailedTransient",
            3 => "InProgress",
            4 => "Finished",
            _ => "Inconnu",
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.position.checked_add(count)
            .ok_or_else(|| "Débordement lors de la lecture de chunks.db".to_string())?;
        if end > self.data.len() {
            return Err(format!("Base XDM tronquée à l'octet {}", self.position));
        }
        let result = &self.data[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn i32(&mut self) -> Result<i32, String> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("taille vérifiée");
        Ok(i32::from_le_bytes(bytes))
    }

    fn i64(&mut self) -> Result<i64, String> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("taille vérifiée");
        Ok(i64::from_le_bytes(bytes))
    }

    fn f64(&mut self) -> Result<f64, String> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("taille vérifiée");
        Ok(f64::from_le_bytes(bytes))
    }

    /// Chaîne .NET `BinaryWriter.Write(string)`: longueur en base 128 puis UTF-8.
    fn dotnet_string(&mut self) -> Result<String, String> {
        let mut length: usize = 0;
        let mut shift = 0_u32;
        let mut finished = false;
        for _ in 0..5 {
            let byte = self.take(1)?[0];
            length |= ((byte & 0x7f) as usize)
                .checked_shl(shift)
                .ok_or_else(|| "Longueur de chaîne invalide dans chunks.db".to_string())?;
            if byte & 0x80 == 0 {
                finished = true;
                break;
            }
            shift += 7;
        }
        if !finished || length > MAX_STRING_SIZE {
            return Err("Longueur de chaîne .NET invalide dans chunks.db".into());
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| "Chaîne UTF-8 invalide dans chunks.db".into())
    }
}

fn parse_db_bytes(raw: &[u8]) -> Result<Vec<Chunk>, String> {
    if raw.len() < 8 {
        return Err("Fichier chunks.db trop petit".into());
    }
    let pointer = i32::from_le_bytes(raw[0..4].try_into().expect("taille vérifiée"));
    if pointer < 4 {
        return Err("Pointeur de fin invalide dans chunks.db".into());
    }
    let end = pointer as usize;
    if end.checked_add(FOOTER.len()).is_none_or(|footer_end| footer_end > raw.len())
        || &raw[end..end + FOOTER.len()] != FOOTER
    {
        return Err("Marqueur END. absent ou invalide dans chunks.db".into());
    }

    let mut reader = Reader::new(&raw[4..end]);
    let count = reader.i32()?;
    if count <= 0 || count as usize > MAX_CHUNKS {
        return Err(format!("Nombre de segments incohérent : {count}"));
    }

    let mut chunks = Vec::with_capacity(count as usize);
    for index in 0..count {
        let id = reader.dotnet_string()?;
        let downloaded = reader.i64()?;
        let size = reader.i64()?;
        let _offset = reader.i64()?;
        let stream_index = reader.i32()?;
        let duration = reader.f64()?;
        let status = reader.i32()?;
        let url = reader.dotnet_string()?;

        if !(0..=1).contains(&stream_index) {
            return Err(format!("Segment {index} : index de piste inattendu {stream_index}"));
        }
        if !duration.is_finite() || duration < 0.0 {
            return Err(format!("Segment {index} : durée invalide"));
        }
        if downloaded < 0 || size < -1 {
            return Err(format!("Segment {index} : taille ou progression invalide"));
        }
        chunks.push(Chunk { id, stream_index, status, url });
    }
    Ok(chunks)
}

fn read_db(folder: &Path) -> Result<(PathBuf, Vec<Chunk>), String> {
    let mut errors = Vec::new();
    for filename in ["chunks.db.1", "chunks.db.2"] {
        let path = folder.join(filename);
        if !path.is_file() {
            continue;
        }
        match fs::read(&path).map_err(|error| error.to_string())
            .and_then(|bytes| parse_db_bytes(&bytes))
        {
            Ok(chunks) => return Ok((path, chunks)),
            Err(error) => errors.push(format!("{filename}: {error}")),
        }
    }
    if errors.is_empty() {
        Err(format!("Aucun chunks.db.1 ou chunks.db.2 trouvé dans {}", folder.display()))
    } else {
        Err(format!("État XDM illisible : {}", errors.join(" ; ")))
    }
}

fn segment_filename(url: &str) -> String {
    let raw_name = Url::parse(url).ok()
        .and_then(|parsed| parsed.path_segments().and_then(|segments| segments.last().map(str::to_owned)))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "segment.ts".to_string());
    let decoded = percent_decode_str(&raw_name).decode_utf8_lossy();
    let safe: String = decoded.chars().map(|character| {
        if character == '/' || character == '\\' || character == '\0' { '_' } else { character }
    }).collect();
    if safe.is_empty() || safe == "." || safe == ".." { "segment.ts".into() } else { safe }
}

fn segment_path(folder: &Path, chunk: &Chunk) -> PathBuf {
    let prefix = if chunk.stream_index == 0 { "1_" } else { "2_" };
    folder.join(format!("{prefix}{}{}", chunk.id, segment_filename(&chunk.url)))
}

fn segment_extension(chunks: &[Chunk]) -> String {
    for chunk in chunks {
        if let Ok(url) = Url::parse(&chunk.url) {
            if let Some(extension) = url.path_segments().and_then(|parts| parts.last())
                .and_then(|name| Path::new(name).extension()).and_then(|ext| ext.to_str())
            {
                if !extension.is_empty() && extension.len() <= 8 && extension.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return extension.to_ascii_lowercase();
                }
            }
        }
    }
    "ts".into()
}

fn ffconcat_quote(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let value = absolute.to_string_lossy().replace('\\', "/");
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn write_manifest(paths: &[PathBuf], manifest_path: &Path) -> Result<(), String> {
    let mut file = fs::File::create(manifest_path)
        .map_err(|error| format!("Impossible d'écrire la liste des segments : {error}"))?;
    writeln!(file, "ffconcat version 1.0")
        .map_err(|error| format!("Écriture du manifeste FFmpeg impossible : {error}"))?;
    for path in paths {
        writeln!(file, "file {}", ffconcat_quote(path))
            .map_err(|error| format!("Écriture du manifeste FFmpeg impossible : {error}"))?;
    }
    Ok(())
}

fn last_stderr_lines(stderr: &[u8]) -> String {
    let value = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = value.lines().rev().take(8).collect();
    if lines.is_empty() { "Aucun détail fourni par FFmpeg.".into() }
    else { lines.into_iter().rev().collect::<Vec<_>>().join("\n") }
}

fn run_concat(paths: &[PathBuf], manifest: &Path, destination: &Path) -> Result<(), String> {
    write_manifest(paths, manifest)?;
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostdin", "-loglevel", "warning", "-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(manifest)
        .args(["-map", "0", "-c", "copy"])
        .arg(destination)
        .output()
        .map_err(|error| format!("Impossible de lancer FFmpeg. Vérifie son installation : {error}"))?;
    if !output.status.success() {
        return Err(format!("FFmpeg a échoué pendant la concaténation :\n{}", last_stderr_lines(&output.stderr)));
    }
    let metadata = fs::metadata(destination)
        .map_err(|error| format!("FFmpeg n'a pas créé la sortie attendue : {error}"))?;
    if metadata.len() == 0 {
        return Err("FFmpeg a créé un fichier vide.".into());
    }
    Ok(())
}

fn run_mux(video: &Path, audio: &Path, destination: &Path, extension: &str) -> Result<(), String> {
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-nostdin", "-loglevel", "warning", "-y", "-i"])
        .arg(video)
        .arg("-i").arg(audio)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]);
    if extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("m4v") {
        command.args(["-movflags", "+faststart"]);
    }
    let output = command.arg(destination).output()
        .map_err(|error| format!("Impossible de lancer FFmpeg pour réunir les pistes : {error}"))?;
    if !output.status.success() {
        return Err(format!("FFmpeg n'a pas pu réunir l'audio et la vidéo :\n{}", last_stderr_lines(&output.stderr)));
    }
    let metadata = fs::metadata(destination)
        .map_err(|error| format!("FFmpeg n'a pas créé le fichier final : {error}"))?;
    if metadata.len() == 0 { return Err("Le fichier final créé est vide.".into()); }
    Ok(())
}

fn validate_target_name(name: &str) -> Result<(String, String), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() { return Err("Donne un nom au fichier cible.".into()); }
    if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\\') {
        return Err("Le nom cible doit être un nom de fichier, pas un chemin.".into());
    }
    let path = Path::new(trimmed);
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp4", "mkv", "ts", "mov", "m4v"].contains(&extension.as_str()) {
        return Err("Extension non prise en charge. Utilise .mp4, .mkv, .ts, .mov ou .m4v.".into());
    }
    Ok((trimmed.to_string(), extension))
}

/// Assemble les segments enregistrés par XDM sans modifier les fichiers source.
pub fn merge(request: MergeRequest) -> Result<MergeReport, String> {
    if !request.source_dir.is_dir() {
        return Err(format!("Le dossier source n'existe pas : {}", request.source_dir.display()));
    }
    if !Command::new("ffmpeg").arg("-version").output().is_ok_and(|output| output.status.success()) {
        return Err("FFmpeg est introuvable. Installe-le et vérifie que `ffmpeg` est dans PATH.".into());
    }
    let (target_name, extension) = validate_target_name(&request.target_name)?;
    fs::create_dir_all(&request.output_dir)
        .map_err(|error| format!("Impossible de créer le dossier de sortie : {error}"))?;
    let destination = request.output_dir.join(target_name);
    if destination.exists() {
        return Err(format!("Le fichier cible existe déjà ; aucun fichier n'a été écrasé : {}", destination.display()));
    }

    let (database, chunks) = read_db(&request.source_dir)?;
    let mut grouped: BTreeMap<i32, Vec<(Chunk, PathBuf)>> = BTreeMap::new();
    let mut incomplete = Vec::new();
    let mut problems = Vec::new();
    let mut segment_count = 0;

    for chunk in chunks {
        let path = segment_path(&request.source_dir, &chunk);
        if !chunk.is_finished() {
            incomplete.push(format!("{} ({})", path.file_name().unwrap_or_default().to_string_lossy(), chunk.status_label()));
        }
        if !path.is_file() {
            problems.push(format!("Segment absent : {}", path.file_name().unwrap_or_default().to_string_lossy()));
            continue;
        }
        let metadata = fs::metadata(&path).map_err(|error| format!("Lecture impossible de {} : {error}", path.display()))?;
        if metadata.len() == 0 {
            problems.push(format!("Segment vide : {}", path.file_name().unwrap_or_default().to_string_lossy()));
            continue;
        }
        if !chunk.is_finished() && !request.include_incomplete { continue; }
        // Conserve l'ordre de la base XDM dans le manifeste FFmpeg.
        segment_count += 1;
        grouped.entry(chunk.stream_index).or_default().push((chunk, path));
    }

    if !request.include_incomplete && (!incomplete.is_empty() || !problems.is_empty()) {
        let mut message = format!("Récupération interrompue en mode sûr. {} segment(s) non terminés ; {} problème(s) de fichiers.", incomplete.len(), problems.len());
        for detail in incomplete.iter().chain(problems.iter()).take(6) { message.push_str("\n• "); message.push_str(detail); }
        message.push_str("\nActive l'option de récupération partielle uniquement si tu acceptes une sortie incomplète.");
        return Err(message);
    }
    if grouped.is_empty() { return Err("Aucun segment lisible à assembler.".into()); }
    let streams: Vec<i32> = grouped.keys().copied().collect();
    if streams != [0] && streams != [0, 1] {
        return Err(format!("Combinaison de pistes XDM non prise en charge : {streams:?}"));
    }

    let partial = !incomplete.is_empty() || !problems.is_empty();
    let output_dir = request.output_dir.canonicalize()
        .map_err(|error| format!("Le dossier de sortie n'est pas accessible : {error}"))?;
    let final_target = output_dir.join(&target_name);
    if final_target.exists() { return Err(format!("Le fichier cible existe déjà : {}", final_target.display())); }

    let temp_dir = TempBuilder::new().prefix(".mergevideoxdm-").tempdir_in(&output_dir)
        .map_err(|error| format!("Impossible de créer le dossier temporaire de traitement : {error}"))?;
    let final_temp = TempBuilder::new().prefix(".mergevideoxdm-final-").suffix(&format!(".{extension}")).tempfile_in(&output_dir)
        .map_err(|error| format!("Impossible de préparer le fichier de sortie : {error}"))?;

    if streams == [0] {
        let track = &grouped[&0];
        let paths: Vec<PathBuf> = track.iter().map(|(_, path)| path.clone()).collect();
        let manifest = temp_dir.path().join("segments.ffconcat");
        run_concat(&paths, &manifest, final_temp.path())?;
    } else {
        let video_chunks = &grouped[&0];
        let audio_chunks = &grouped[&1];
        let video_paths: Vec<PathBuf> = video_chunks.iter().map(|(_, path)| path.clone()).collect();
        let audio_paths: Vec<PathBuf> = audio_chunks.iter().map(|(_, path)| path.clone()).collect();
        let video = temp_dir.path().join(format!("video.{}", segment_extension(&video_chunks.iter().map(|(chunk, _)| chunk.clone()).collect::<Vec<_>>())));
        let audio = temp_dir.path().join(format!("audio.{}", segment_extension(&audio_chunks.iter().map(|(chunk, _)| chunk.clone()).collect::<Vec<_>>())));
        run_concat(&video_paths, &temp_dir.path().join("video.ffconcat"), &video)?;
        run_concat(&audio_paths, &temp_dir.path().join("audio.ffconcat"), &audio)?;
        run_mux(&video, &audio, final_temp.path(), &extension)?;
    }

    let persisted = final_temp.persist_noclobber(&final_target)
        .map_err(|error| format!("FFmpeg a terminé, mais impossible d'enregistrer la cible sans écrasement : {}", error.error))?;
    drop(persisted);

    let mut detail = format!("{} segment(s) assemblé(s) à partir de {}", segment_count, database.file_name().unwrap_or_default().to_string_lossy());
    if partial { detail.push_str(". Récupération partielle : certains segments sont incomplets ou absents"); }
    Ok(MergeReport { output: final_target, segment_count, partial, detail })
}

/// Retourne la première ligne de `ffmpeg -version`, ou une erreur lisible.
pub fn ffmpeg_info() -> Result<String, String> {
    let output = Command::new("ffmpeg").arg("-version").output()
        .map_err(|_| "FFmpeg non détecté dans PATH".to_string())?;
    if !output.status.success() { return Err("La commande FFmpeg existe mais ne répond pas correctement".into()); }
    let version = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("FFmpeg détecté").to_string();
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_i32(out: &mut Vec<u8>, value: i32) { out.extend(value.to_le_bytes()); }
    fn write_i64(out: &mut Vec<u8>, value: i64) { out.extend(value.to_le_bytes()); }
    fn write_f64(out: &mut Vec<u8>, value: f64) { out.extend(value.to_le_bytes()); }
    fn write_dotnet_string(out: &mut Vec<u8>, value: &str) {
        let bytes = value.as_bytes();
        let mut length = bytes.len();
        while length >= 0x80 { out.push((length as u8) | 0x80); length >>= 7; }
        out.push(length as u8);
        out.extend(bytes);
    }

    #[test]
    fn parses_xdm_829_state_and_keeps_playlist_order() {
        let mut bytes = vec![0, 0, 0, 0];
        write_i32(&mut bytes, 2);
        for (id, url) in [("second", "https://example.test/seg02.ts"), ("first", "https://example.test/seg01.ts")] {
            write_dotnet_string(&mut bytes, id);
            write_i64(&mut bytes, 120);
            write_i64(&mut bytes, 120);
            write_i64(&mut bytes, 0);
            write_i32(&mut bytes, 0);
            write_f64(&mut bytes, 1.25);
            write_i32(&mut bytes, 4);
            write_dotnet_string(&mut bytes, url);
        }
        let footer_pos = bytes.len() as i32;
        bytes.extend_from_slice(FOOTER);
        bytes[0..4].copy_from_slice(&footer_pos.to_le_bytes());
        let chunks = parse_db_bytes(&bytes).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].id, "second");
        assert_eq!(chunks[1].id, "first");
        assert_eq!(chunks[0].stream_index, 0);
    }

    #[test]
    fn constructs_segment_path_with_decoded_filename() {
        let chunk = Chunk {
            id: "abc-".into(), stream_index: 0, status: 4,
            url: "https://example.test/part%2001.ts?token=ignored".into(),
        };
        assert_eq!(segment_path(Path::new("/tmp/xdm"), &chunk), PathBuf::from("/tmp/xdm/1_abc-part 01.ts"));
    }

    #[test]
    fn rejects_path_traversal_in_target_name() {
        assert!(validate_target_name("../outside.mp4").is_err());
        assert!(validate_target_name("video.mp4").is_ok());
        assert!(validate_target_name("video.exe").is_err());
    }
}
