//! Lecteur dédié au format binaire XDM 8.0.29.
use std::path::Path;

use super::{XdmSegment, XdmState, XdmStateAdapter};
use crate::errors::{AppError, ErrorKind};

const FOOTER: &[u8; 4] = b"END.";
const MAX_STRING_SIZE: usize = 64 * 1024 * 1024;
const MAX_SEGMENTS: usize = 2_000_000;

pub struct Adapter;

impl XdmStateAdapter for Adapter {
    fn version(&self) -> &'static str {
        "8.0.29"
    }

    fn recognizes(&self, filename: &str, bytes: &[u8]) -> bool {
        if !matches!(filename, "chunks.db.1" | "chunks.db.2") || bytes.len() < 8 {
            return false;
        }
        let pointer = i32::from_le_bytes(bytes[0..4].try_into().expect("taille vérifiée"));
        if pointer < 4 {
            return false;
        }
        let end = pointer as usize;
        end.checked_add(FOOTER.len())
            .is_some_and(|footer_end| footer_end <= bytes.len() && &bytes[end..footer_end] == FOOTER)
    }

    fn parse(&self, path: &Path, bytes: &[u8]) -> Result<XdmState, AppError> {
        parse_state(path, bytes)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], AppError> {
        let end = self.position.checked_add(count).ok_or_else(|| {
            AppError::new(ErrorKind::CorruptXdmState, "Débordement pendant la lecture binaire.")
        })?;
        if end > self.bytes.len() {
            return Err(AppError::new(
                ErrorKind::CorruptXdmState,
                format!("Base XDM tronquée à l'octet {}.", self.position),
            ));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn i32(&mut self) -> Result<i32, AppError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("taille vérifiée")))
    }

    fn i64(&mut self) -> Result<i64, AppError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().expect("taille vérifiée")))
    }

    fn f64(&mut self) -> Result<f64, AppError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().expect("taille vérifiée")))
    }

    /// Décodage de BinaryWriter.Write(string) : longueur UTF-8 encodée en base 128.
    fn dotnet_string(&mut self) -> Result<String, AppError> {
        let mut length = 0_usize;
        let mut shift = 0_u32;
        let mut terminated = false;

        for _ in 0..5 {
            let byte = self.take(1)?[0];
            let part = ((byte & 0x7f) as usize)
                .checked_shl(shift)
                .ok_or_else(|| AppError::new(ErrorKind::CorruptXdmState, "Longueur de chaîne invalide."))?;
            length |= part;
            if byte & 0x80 == 0 {
                terminated = true;
                break;
            }
            shift += 7;
        }

        if !terminated || length > MAX_STRING_SIZE {
            return Err(AppError::new(ErrorKind::CorruptXdmState, "Longueur de chaîne .NET invalide."));
        }

        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| {
            AppError::new(ErrorKind::CorruptXdmState, "Chaîne UTF-8 invalide dans la base XDM.")
        })
    }
}

fn parse_state(path: &Path, bytes: &[u8]) -> Result<XdmState, AppError> {
    if bytes.len() < 8 {
        return Err(AppError::new(ErrorKind::CorruptXdmState, "Fichier d'état XDM trop petit."));
    }

    let pointer = i32::from_le_bytes(bytes[0..4].try_into().expect("taille vérifiée"));
    if pointer < 4 {
        return Err(AppError::new(ErrorKind::CorruptXdmState, "Pointeur de fin invalide."));
    }
    let end = pointer as usize;
    if end.checked_add(FOOTER.len()).is_none_or(|footer_end| footer_end > bytes.len())
        || &bytes[end..end + FOOTER.len()] != FOOTER
    {
        return Err(AppError::new(ErrorKind::CorruptXdmState, "Marqueur END. absent ou invalide."));
    }

    let mut reader = Reader::new(&bytes[4..end]);
    let count = reader.i32()?;
    if count <= 0 || count as usize > MAX_SEGMENTS {
        return Err(AppError::new(
            ErrorKind::CorruptXdmState,
            format!("Nombre de segments incohérent : {count}."),
        ));
    }

    let mut segments = Vec::with_capacity(count as usize);
    for index in 0..count {
        let id = reader.dotnet_string()?;
        let downloaded = reader.i64()?;
        let size = reader.i64()?;
        let _offset = reader.i64()?;
        let stream_index = reader.i32()?;
        let duration_seconds = reader.f64()?;
        let status = reader.i32()?;
        let url = reader.dotnet_string()?;

        if !(0..=1).contains(&stream_index) {
            return Err(AppError::new(
                ErrorKind::CorruptXdmState,
                format!("Segment {index} : index de piste inattendu ({stream_index})."),
            ));
        }
        if !duration_seconds.is_finite() || duration_seconds < 0.0 {
            return Err(AppError::new(ErrorKind::CorruptXdmState, format!("Segment {index} : durée invalide.")));
        }
        if downloaded < 0 || size < -1 {
            return Err(AppError::new(
                ErrorKind::CorruptXdmState,
                format!("Segment {index} : tailles ou progression invalides."),
            ));
        }

        segments.push(XdmSegment { id, url, stream_index, status, duration_seconds });
    }

    Ok(XdmState {
        version: "8.0.29".into(),
        state_file: path.to_path_buf(),
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_i32(output: &mut Vec<u8>, value: i32) {
        output.extend(value.to_le_bytes());
    }
    fn write_i64(output: &mut Vec<u8>, value: i64) {
        output.extend(value.to_le_bytes());
    }
    fn write_f64(output: &mut Vec<u8>, value: f64) {
        output.extend(value.to_le_bytes());
    }
    fn write_dotnet_string(output: &mut Vec<u8>, value: &str) {
        let mut length = value.len();
        while length >= 0x80 {
            output.push((length as u8) | 0x80);
            length >>= 7;
        }
        output.push(length as u8);
        output.extend(value.as_bytes());
    }
    fn sample_state() -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 0];
        write_i32(&mut bytes, 2);
        for (id, url) in [
            ("second", "https://example.test/seg02.ts"),
            ("first", "https://example.test/seg01.ts"),
        ] {
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
        bytes
    }

    #[test]
    fn recognizes_and_parses_xdm_829_state() {
        let bytes = sample_state();
        let adapter = Adapter;
        assert!(adapter.recognizes("chunks.db.1", &bytes));
        let state = adapter.parse(Path::new("/tmp/chunks.db.1"), &bytes).unwrap();
        assert_eq!(state.version, "8.0.29");
        assert_eq!(state.segments.len(), 2);
        assert_eq!(state.segments[0].id, "second");
        assert_eq!(state.segments[1].id, "first");
    }

    #[test]
    fn rejects_invalid_footer() {
        let mut bytes = sample_state();
        let length = bytes.len();
        bytes[length - 1] = b'!';
        assert!(!Adapter.recognizes("chunks.db.1", &bytes));
    }

    #[test]
    fn does_not_claim_other_database_names() {
        assert!(!Adapter.recognizes("main.db", &sample_state()));
    }
}
