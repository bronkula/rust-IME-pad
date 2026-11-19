//! Dataset generator: kanjiVG SVG files -> compact KVPD binary.
//!
//! Usage: cargo run --bin gen -- <kanjiVG-dir> <out-file>
//!
//! kanjiVG data is licensed CC BY-SA 3.0 (https://kanjivg.tagaini.net).

use kanji_pad::db::write_db;
use kanji_pad::engine::{Pt, entry_from_raw};
use kanji_pad::path::{extract_path_data, parse_path};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kanji_dir = args
        .get(1)
        .cloned()
        .unwrap_or("C:/Users/Hamilton/AppData/Local/Temp/kanjiVG".to_string());
    let out = args
        .get(2)
        .cloned()
        .unwrap_or("assets/kanji.kvpd".to_string());

    let index_path = format!("{kanji_dir}/kvg-index.json");
    let index_str = std::fs::read_to_string(&index_path)
        .unwrap_or_else(|e| panic!("read {index_path}: {e}"));
    let index: serde_json::Value = serde_json::from_str(&index_str)
        .unwrap_or_else(|e| panic!("parse {index_path}: {e}"));

    let obj = index.as_object().expect("index must be a JSON object");
    let mut entries = Vec::new();
    let mut skipped = 0usize;

    for (ch, files) in obj {
        let Some(c) = ch.chars().next() else { continue };
        if ch.chars().count() != 1 {
            continue;
        }
        // Japanese-relevant ranges: kana + CJK ideographs.
        let keep = matches!(c, '\u{3041}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}');
        if !keep {
            continue;
        }
        let Some(f) = files.as_array().and_then(|a| a[0].as_str()) else {
            continue;
        };
        let svg_path = format!("{kanji_dir}/kanji/{f}");
        let Ok(svg) = std::fs::read_to_string(&svg_path) else {
            skipped += 1;
            if skipped <= 3 {
                eprintln!("read failed: {svg_path}");
            }
            continue;
        };
        let raw: Vec<Vec<Pt>> = extract_path_data(&svg)
            .iter()
            .map(|d| -> Vec<Pt> {
                parse_path(d)
                    .into_iter()
                    .map(|(x, y)| (x as f32, y as f32))
                    .collect()
            })
            .filter(|s| s.len() >= 2)
            .collect();
        if raw.is_empty() {
            skipped += 1;
            if skipped <= 3 {
                eprintln!("no strokes parsed: {svg_path} ({} d attrs)", extract_path_data(&svg).len());
            }
            continue;
        }
        entries.push(entry_from_raw(c, &raw));
    }

    entries.sort_by_key(|e| e.ch);

    let bytes = write_db(&entries, kanji_pad::engine::PPS);
    if let Some(dir) = std::path::Path::new(&out).parent() {
        if dir != std::path::Path::new("") {
            std::fs::create_dir_all(dir).ok();
        }
    }
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("write {out}: {e}"));

    println!(
        "wrote {out}: {} entries, {} bytes (skipped {skipped})",
        entries.len(),
        bytes.len()
    );
}
