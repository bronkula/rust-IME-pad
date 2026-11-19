# Kanji Pad

A standalone Windows app for handwriting Japanese kanji (and kana). Draw strokes
on a square canvas and ranked candidates appear live after each stroke; click a
candidate to copy the character to the clipboard and append it to the text bar
at the top of the window.

It is a _pad_, not a system IME: nothing is injected into other applications.
You draw, pick candidates, and paste the composed text wherever you need it.

## How it works

- **Dataset**: stroke reference data generated from [kanjiVG](https://kanjivg.tagaini.net)
  (kana U+3041–U+30FF plus CJK U+4E00–U+9FFF, 6795 characters) into a compact
  binary format (`assets/kanji.kvpd`, ~10 MB, embedded in the binary).
- **Grid space**: kanjiVG authors every glyph on a fixed `0 0 109 109` viewBox,
  so absolute stroke positions are meaningful. The canvas square maps to that
  grid, and recognition compares strokes in grid space rather than per-character
  bounding boxes — this is what makes incremental stroke-order matching work.
- **Recognition**: each drawn stroke is resampled by arc length and compared to
  the reference stroke at the _same index_ (stroke order is meaningful in a pad,
  so strokes are never permuted). Distance is exact Euclidean DTW with early
  abort. Characters with more strokes than drawn are penalised in ranking but
  kept in the pool if their drawn prefix is close, so they can still complete as
  you add strokes. After each stroke the candidate pool narrows to the previous
  pool's characters.
- **Stroke-count flexibility**: if no character matches the strokes as drawn,
  alternative readings are tried — adjacent strokes whose shared endpoints
  nearly touch are merged into one reference stroke (the pen was lifted
  mid-stroke), and a stroke with a sharp corner is split into two reference
  strokes (two strokes were drawn as one). All readings share one distance
  budget, so extra variants cost little.
- **Scripts**: hiragana, katakana, and kanji are classified by codepoint range,
  so kana and kanji both come back from the same canvas; every script is always
  recognized.
- **Canvas**: a ~300×300 px square with a faint 田字格-style cross guide
  dividing it into four cells, like a kanji practice grid.
- **Suggestions**: candidates appear as square character buttons in a
  scrollable grid below the Undo/Clear icon row; the best guess is highlighted.
  An overlay draws the remaining strokes of the best guess on the canvas as a
  ghost guide. Click a button to copy the character to the clipboard and append
  it to the top text bar.
- **Text bar**: a single-line input across the top of the window collects the
  characters you click, so a whole group can be selected and copied at once.

## Build

```sh
cargo build --release
cargo run --release
```

The dataset is committed in `assets/kanji.kvpd`, so a plain build works.

### Regenerating the dataset

Clone kanjiVG and run the generator:

```sh
git clone https://github.com/kanjiVG/kanjiVG
cargo run --bin gen -- path/to/kanjiVG assets/kanji.kvpd
```

## Tests

```sh
cargo test
```

Covers the SVG path parser, resampling, grid normalization, DTW (including
early-abort correctness), Hungarian assignment against brute force, the binary
database round-trip, merge/split alignment variants, and dataset-backed
recognition: single stroke → 一, incremental two strokes → 二, 一 drawn as two
touching strokes → 一 via merging, replayed kana strokes → あ/ひ/ず and
ア/ニ/シ, and script-mask filtering.

## Input

Mouse, touch, and pen all work — egui pointer events cover them. Pen pressure
is not used.

## Clipboard

Clicking a candidate copies the single character to the clipboard via
`arboard` and appends it to the top text bar, where the accumulated group can
be selected and copied in one go.

## Fonts

Kanji glyphs render using a system CJK font (NotoSansJP, Microsoft YaHei, or
Yu Gothic). If none is found, the UI warns and glyphs may not render.

## Attribution

Stroke data comes from **kanjiVG**, Copyright © 2009–2011 Ulrich Apel,
distributed under the Creative Commons Attribution-Share Alike 3.0 licence
(https://kanjivg.tagaini.net).
