//! Compact binary stroke database: quantized normalized stroke points.

use crate::engine::Pt;

pub const MAGIC: &[u8; 4] = b"KVPD";
pub const VERSION: u32 = 1;
/// Quantization scale: coordinates in [0,1] stored as 0..1000.
const SCALE: f32 = 1000.0;

/// The generated dataset asset, available to tests.
#[cfg(test)]
pub const DB_BYTES_FOR_TESTS: &[u8] = include_bytes!("../assets/kanji.kvpd");

#[derive(Debug, Clone)]
pub struct Entry {
    pub ch: char,
    /// Normalized strokes, coordinates in [0,1].
    pub strokes: Vec<Vec<Pt>>,
}

#[derive(Debug, Clone)]
pub struct KanjiDb {
    pub points_per_stroke: usize,
    pub entries: Vec<Entry>,
}

pub fn write_db(entries: &[Entry], pps: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(pps as u32).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&(e.ch as u32).to_le_bytes());
        out.extend_from_slice(&(e.strokes.len() as u32).to_le_bytes());
        for st in &e.strokes {
            assert_eq!(st.len(), pps, "stroke must have {pps} points");
            for &(x, y) in st {
                let qx = (x * SCALE).round().clamp(0.0, SCALE) as u16;
                let qy = (y * SCALE).round().clamp(0.0, SCALE) as u16;
                out.extend_from_slice(&qx.to_le_bytes());
                out.extend_from_slice(&qy.to_le_bytes());
            }
        }
    }
    out
}

pub fn read_db(bytes: &[u8]) -> Result<KanjiDb, String> {
    if bytes.len() < 16 {
        return Err("too short".into());
    }
    if &bytes[0..4] != MAGIC {
        return Err("bad magic".into());
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != VERSION {
        return Err(format!("unsupported version {version}"));
    }
    let pps = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;

    let mut pos = 16;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if pos + 8 > bytes.len() {
            return Err("truncated".into());
        }
        let cp = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        let strokes = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        pos += 8;
        let need = strokes * pps * 4;
        if pos + need > bytes.len() {
            return Err("truncated stroke data".into());
        }
        let mut all = Vec::with_capacity(strokes);
        for _ in 0..strokes {
            let mut st = Vec::with_capacity(pps);
            for _ in 0..pps {
                let qx = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
                let qy = u16::from_le_bytes(bytes[pos + 2..pos + 4].try_into().unwrap());
                pos += 4;
                st.push((qx as f32 / SCALE, qy as f32 / SCALE));
            }
            all.push(st);
        }
        let ch = char::from_u32(cp).ok_or("invalid codepoint")?;
        entries.push(Entry { ch, strokes: all });
    }
    Ok(KanjiDb {
        points_per_stroke: pps,
        entries,
    })
}

impl KanjiDb {
    /// Reference strokes for a character, if present. Entries are sorted by
    /// character, so this is a binary search.
    pub fn strokes_for(&self, ch: char) -> Option<&[Vec<Pt>]> {
        self.entries
            .binary_search_by_key(&ch, |e| e.ch)
            .ok()
            .map(|i| self.entries[i].strokes.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(a: Pt, b: Pt) -> Vec<Pt> {
        (0..32)
            .map(|k| {
                let t = k as f32 / 31.0;
                (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
            })
            .collect()
    }

    #[test]
    fn round_trip() {
        let entries = vec![
            Entry {
                ch: '一',
                strokes: vec![stroke((0.0, 0.5), (1.0, 0.5)), stroke((0.2, 0.2), (0.8, 0.8))],
            },
            Entry {
                ch: 'あ',
                strokes: vec![stroke((0.5, 0.0), (0.5, 1.0))],
            },
        ];
        let bytes = write_db(&entries, 32);
        let db = read_db(&bytes).unwrap();
        assert_eq!(db.points_per_stroke, 32);
        assert_eq!(db.entries.len(), 2);
        assert_eq!(db.entries[0].ch, '一');
        assert_eq!(db.entries[1].ch, 'あ');
        assert_eq!(db.entries[0].strokes.len(), 2);
        assert_eq!(db.entries[0].strokes[0][0], (0.0, 0.5));
        assert_eq!(db.entries[0].strokes[0][31], (1.0, 0.5));
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(read_db(b"XXXX123456789012").is_err());
    }
}
