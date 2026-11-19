//! Stroke recognition engine: resampling, normalization, DTW, Hungarian assignment.

use crate::db::{Entry, KanjiDb};

pub type Pt = (f32, f32);

/// Points per stroke after arc-length resampling.
pub const PPS: usize = 32;

/// kanjiVG authors every glyph on a fixed `0 0 109 109` viewBox grid, so
/// absolute stroke positions are meaningful. Recognition compares strokes in
/// this grid space (SVG coords divided by GRID), which is what incremental
/// stroke-order matching needs.
pub const GRID: f32 = 109.0;

/// Penalty per extra reference stroke beyond the drawn stroke count. Large
/// enough that a character whose stroke count matches the drawn strokes wins
/// over one whose first strokes merely resemble them.
const PENALTY: f32 = 0.1;

/// How much worse than the best prefix a character may be and still be kept
/// in the pool for incremental narrowing (its later strokes may complete it).
const PREFIX_MARGIN: f32 = 0.35;

/// How close the end of one drawn stroke must be to the start of the next for
/// them to be considered one reference stroke that the user lifted the pen in
/// the middle of.
const MERGE_DIST: f32 = 0.08;

/// Turn angle above which a vertex counts as a corner where two reference
/// strokes were drawn as a single stroke.
const SPLIT_ANGLE: f32 = 60.0f32.to_radians();

/// Upper bound on alignment variants considered per recognition pass.
const MAX_VARIANTS: usize = 10;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Resample a polyline to `n` points spaced evenly by arc length.
pub fn resample(pts: &[Pt], n: usize) -> Vec<Pt> {
    if pts.is_empty() {
        return vec![(0.0, 0.0); n];
    }
    if n == 0 {
        return vec![];
    }
    let mut cum = vec![0.0f32];
    for w in pts.windows(2) {
        let d = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
        cum.push(cum[cum.len() - 1] + d);
    }
    let total = cum[cum.len() - 1];
    if total <= 1e-6 {
        return vec![pts[0]; n];
    }
    let mut out = Vec::with_capacity(n);
    let mut seg = 0usize;
    for k in 0..n {
        let t = total * k as f32 / (n - 1) as f32;
        while seg + 1 < cum.len() && cum[seg + 1] < t {
            seg += 1;
        }
        if seg + 1 >= cum.len() {
            out.push(pts[pts.len() - 1]);
            continue;
        }
        let d = cum[seg + 1] - cum[seg];
        let f = if d > 1e-6 { (t - cum[seg]) / d } else { 0.0 };
        out.push((lerp(pts[seg].0, pts[seg + 1].0, f), lerp(pts[seg].1, pts[seg + 1].1, f)));
    }
    out
}

/// Map SVG-space strokes (0..GRID) into the unit grid square (0..1) by
/// dividing by the viewBox size. This preserves absolute stroke positions,
/// so a partial stroke keeps its location within the character.
pub fn to_grid(strokes: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    strokes
        .iter()
        .map(|st| st.iter().map(|&(x, y)| (x / GRID, y / GRID)).collect())
        .collect()
}

/// Exact 2D Euclidean DTW with early abort: returns `None` when the distance
/// provably exceeds `limit`.
pub fn dtw(a: &[Pt], b: &[Pt], limit: f32) -> Option<f32> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return None;
    }
    let mut prev = vec![f32::MAX; m];
    let mut cur = vec![0.0f32; m];
    for i in 0..n {
        let mut row_min = f32::MAX;
        for j in 0..m {
            let d = ((a[i].0 - b[j].0).powi(2) + (a[i].1 - b[j].1).powi(2)).sqrt();
            let best = if i == 0 && j == 0 {
                0.0
            } else if i == 0 {
                cur[j - 1]
            } else if j == 0 {
                prev[j]
            } else {
                prev[j].min(prev[j - 1]).min(cur[j - 1])
            };
            cur[j] = d + best;
            row_min = row_min.min(cur[j]);
        }
        if row_min > limit {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Some(prev[m - 1])
}

/// Hungarian algorithm (Kuhn-Munkres, O(n^3)) for a square cost matrix.
/// Returns the column assigned to each row.
pub fn hungarian(a: &[Vec<f64>]) -> Vec<usize> {
    let n = a.len();
    let m = a[0].len();
    let mut u = vec![0.0f64; n + 1];
    let mut v = vec![0.0f64; m + 1];
    let mut p = vec![0usize; m + 1];
    let mut way = vec![0usize; m + 1];

    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0usize;
            for j in 1..=m {
                if !used[j] {
                    let cur = a[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }

    let mut ans = vec![0usize; n];
    for j in 1..=m {
        if p[j] > 0 {
            ans[p[j] - 1] = j - 1;
        }
    }
    ans
}

/// Angle (radians) between the incoming and outgoing segments at vertex `b`.
/// 0 = straight through, pi = full reversal.
fn turn_angle(a: Pt, b: Pt, c: Pt) -> f32 {
    let v1 = (b.0 - a.0, b.1 - a.1);
    let v2 = (c.0 - b.0, c.1 - b.1);
    let l1 = v1.0.hypot(v1.1);
    let l2 = v2.0.hypot(v2.1);
    if l1 < 1e-4 || l2 < 1e-4 {
        return 0.0;
    }
    ((v1.0 * v2.0 + v1.1 * v2.1) / (l1 * l2)).clamp(-1.0, 1.0).acos()
}

/// Vertices where a stroke bends sharply: likely places where two reference
/// strokes were drawn as one continuous stroke. Splitting at `j` yields
/// `stroke[..=j]` and `stroke[j..]`, both at least 2 points.
fn corner_indices(stroke: &[Pt]) -> Vec<usize> {
    if stroke.len() < 3 {
        return vec![];
    }
    (1..stroke.len() - 1)
        .filter(|&j| turn_angle(stroke[j - 1], stroke[j], stroke[j + 1]) > SPLIT_ANGLE)
        .collect()
}

/// Alternative readings of the drawn strokes, so a mismatch in stroke count
/// does not lose the character:
/// - the strokes as drawn;
/// - adjacent strokes whose endpoints nearly touch merged into one (pen lifted
///   mid-stroke), individually and as chained runs;
/// - a stroke split at a sharp corner (two strokes drawn as one).
///
/// The variant count is capped; the identity reading is always first.
pub fn alignment_variants(drawn: &[Vec<Pt>]) -> Vec<Vec<Vec<Pt>>> {
    let mut out: Vec<Vec<Vec<Pt>>> = vec![drawn.to_vec()];
    if drawn.is_empty() {
        return out;
    }

    let close_pairs: Vec<usize> = (0..drawn.len() - 1)
        .filter(|&i| {
            let a = &drawn[i];
            let b = &drawn[i + 1];
            if a.is_empty() || b.is_empty() {
                return false;
            }
            let end = a[a.len() - 1];
            (end.0 - b[0].0).hypot(end.1 - b[0].1) <= MERGE_DIST
        })
        .collect();

    // Merge each close pair on its own.
    for &i in &close_pairs {
        if out.len() >= MAX_VARIANTS {
            break;
        }
        let mut v = Vec::with_capacity(drawn.len() - 1);
        for (j, s) in drawn.iter().enumerate() {
            if j == i {
                let mut merged = s.clone();
                merged.extend(drawn[j + 1].iter().copied());
                v.push(merged);
            } else if j == i + 1 {
                continue;
            } else {
                v.push(s.clone());
            }
        }
        out.push(v);
    }

    // Merge every close pair into runs: the pen may have been lifted several
    // times within one reference stroke.
    if !close_pairs.is_empty() && out.len() < MAX_VARIANTS {
        let mut v: Vec<Vec<Pt>> = Vec::new();
        let mut run = drawn[0].clone();
        for i in 0..drawn.len() - 1 {
            if close_pairs.contains(&i) {
                run.extend(drawn[i + 1].iter().copied());
            } else {
                v.push(std::mem::take(&mut run));
                run = drawn[i + 1].clone();
            }
        }
        v.push(run);
        out.push(v);
    }

    // Split strokes at sharp corners.
    for i in 0..drawn.len() {
        for j in corner_indices(&drawn[i]) {
            if out.len() >= MAX_VARIANTS {
                break;
            }
            let mut v = Vec::with_capacity(drawn.len() + 1);
            for (k, s) in drawn.iter().enumerate() {
                if k == i {
                    v.push(s[..=j].to_vec());
                    v.push(s[j..].to_vec());
                } else {
                    v.push(s.clone());
                }
            }
            out.push(v);
        }
    }

    out
}

/// Which Japanese script a character belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Hiragana,
    Katakana,
    Kanji,
    Other,
}

pub fn script_of(ch: char) -> Script {
    match ch as u32 {
        0x3041..=0x309F => Script::Hiragana,
        0x30A0..=0x30FF => Script::Katakana,
        0x4E00..=0x9FFF => Script::Kanji,
        _ => Script::Other,
    }
}

impl Script {
    pub fn label(self) -> &'static str {
        match self {
            Script::Hiragana => "hiragana",
            Script::Katakana => "katakana",
            Script::Kanji => "kanji",
            Script::Other => "other",
        }
    }
}

/// Which scripts recognition is allowed to return.
#[derive(Debug, Clone, Copy)]
pub struct ScriptMask {
    pub hiragana: bool,
    pub katakana: bool,
    pub kanji: bool,
}

impl ScriptMask {
    pub const ALL: ScriptMask = ScriptMask {
        hiragana: true,
        katakana: true,
        kanji: true,
    };

    pub fn allows(self, s: Script) -> bool {
        match s {
            Script::Hiragana => self.hiragana,
            Script::Katakana => self.katakana,
            Script::Kanji => self.kanji,
            Script::Other => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub ch: char,
    /// Total stroke count of the reference character.
    pub strokes: usize,
    /// Shape distance of the drawn prefix, in grid units (0 = perfect).
    pub dist: f32,
    /// `dist` plus a penalty for reference strokes not yet drawn. Lower is better.
    pub score: f32,
}

impl Candidate {
    pub fn script(&self) -> Script {
        script_of(self.ch)
    }

    /// How closely the drawn strokes match this character's prefix, 0..1.
    pub fn confidence(&self) -> f32 {
        (1.0 - self.dist).clamp(0.0, 1.0)
    }

    /// Strokes still needed to complete this character.
    pub fn remaining(&self, drawn: usize) -> usize {
        self.strokes.saturating_sub(drawn)
    }
}

/// Incremental prefix matching: compare the drawn strokes against the first
/// `n` strokes of every character whose stroke count is at least `n`, in
/// stroke order (drawn stroke `i` against reference stroke `i`). Stroke order
/// is meaningful in a pad, so we do not permute strokes.
///
/// Both sides live in grid space: reference strokes are stored as SVG coords
/// divided by GRID, and drawn strokes come from the canvas square mapped to
/// 0..1, which is the same grid.
///
/// When `prev` is provided (candidates from the previous stroke count), only
/// those characters are re-scored, which keeps incremental matching cheap.
/// `mask` restricts which scripts may appear in the results.
///
/// If no character matches the strokes as drawn closely enough, alternative
/// readings are tried: adjacent strokes whose endpoints nearly touch are
/// treated as one reference stroke (pen lifted mid-stroke), and strokes with
/// a sharp corner are treated as two reference strokes drawn as one.
pub fn recognize(
    drawn: &[Vec<Pt>],
    db: &KanjiDb,
    prev: Option<&[Candidate]>,
    mask: ScriptMask,
    limit: usize,
) -> Vec<Candidate> {
    let drawn: Vec<Vec<Pt>> = drawn.iter().filter(|s| s.len() >= 2).map(|s| s.clone()).collect();
    if drawn.is_empty() {
        return vec![];
    }
    let pps = db.points_per_stroke.max(2);
    let pps_f = pps as f32;
    // Drawn strokes are already in grid space (canvas square = 0..1); only
    // resample them to the reference point count. Resample each alignment
    // variant once, up front.
    let variants = alignment_variants(&drawn);
    let norms: Vec<Vec<Vec<Pt>>> = variants
        .iter()
        .map(|v| v.iter().map(|s| resample(s, pps)).collect())
        .collect();

    let allowed: Option<std::collections::HashSet<char>> =
        prev.map(|p| p.iter().map(|c| c.ch).collect());

    let mut best_dist = f32::INFINITY;
    let mut out: Vec<Candidate> = Vec::new();

    // Variant 0 is the strokes exactly as drawn; the rest are alternative
    // readings (merged or split strokes). All share one running `best_dist`,
    // so the early-abort budget prunes later variants against what the plain
    // reading already found — extra variants cost little.
    for norm in &norms {
        let n = norm.len();
        for e in &db.entries {
            if let Some(set) = &allowed {
                if !set.contains(&e.ch) {
                    continue;
                }
            }
            if !mask.allows(script_of(e.ch)) {
                continue;
            }
            let k = e.strokes.len();
            if k < n {
                continue;
            }
            // Retain any character whose drawn-prefix shape is close, even if
            // it has more strokes than drawn: those are the ones that can
            // still match after further strokes. The stroke-count penalty only
            // affects ranking.
            let cutoff = best_dist + PREFIX_MARGIN;
            // dtw returns a sum over `pps` points; work in average-point units.
            let budget = cutoff * n as f32;
            let mut total = 0.0f32;
            let mut skip = false;
            for i in 0..n {
                let remaining = budget - total;
                if remaining <= 0.0 {
                    skip = true;
                    break;
                }
                match dtw(&norm[i], &e.strokes[i], remaining * pps_f) {
                    Some(d) => total += d / pps_f,
                    None => {
                        skip = true;
                        break;
                    }
                }
            }
            if skip {
                continue;
            }

            let dist = total / n as f32;
            best_dist = best_dist.min(dist);
            let penalty = PENALTY * (k - n) as f32;
            out.push(Candidate {
                ch: e.ch,
                strokes: k,
                dist,
                score: dist + penalty,
            });
        }
    }

    out.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));
    // A character may be scored by several alignments; keep its best score.
    let mut seen: std::collections::HashSet<char> = std::collections::HashSet::new();
    out.retain(|c| seen.insert(c.ch));
    out.truncate(limit);
    out
}

/// Convenience: build an entry from raw stroke polylines (SVG-space points).
pub fn entry_from_raw(ch: char, raw: &[Vec<Pt>]) -> Entry {
    let resampled: Vec<Vec<Pt>> = raw.iter().map(|s| resample(s, PPS)).collect();
    Entry {
        ch,
        strokes: to_grid(&resampled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Pt> {
        let n = 32;
        (0..n)
            .map(|k| {
                let t = k as f32 / (n - 1) as f32;
                (lerp(x0, x1, t), lerp(y0, y1, t))
            })
            .collect()
    }

    #[test]
    fn resample_even_spacing() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0)];
        let r = resample(&pts, 11);
        assert_eq!(r.len(), 11);
        assert_eq!(r[0], (0.0, 0.0));
        assert_eq!(r[5], (5.0, 0.0));
        assert_eq!(r[10], (10.0, 0.0));
    }

    #[test]
    fn resample_degenerate() {
        let r = resample(&[(3.0, 4.0)], 8);
        assert_eq!(r, vec![(3.0, 4.0); 8]);
    }

    #[test]
    fn to_grid_maps_viewbox() {
        let strokes = vec![vec![(0.0, 0.0), (109.0, 54.5)]];
        let norm = to_grid(&strokes);
        assert_eq!(norm[0][0], (0.0, 0.0));
        assert_eq!(norm[0][1], (1.0, 0.5));
    }

    #[test]
    fn dtw_identical_zero() {
        let a = line(0.0, 0.0, 1.0, 0.0);
        assert_eq!(dtw(&a, &a, f32::MAX), Some(0.0));
    }

    #[test]
    fn dtw_offset_line() {
        let a = line(0.0, 0.0, 1.0, 0.0);
        let b = line(0.0, 0.2, 1.0, 0.2);
        let d = dtw(&a, &b, f32::MAX).unwrap();
        assert!((d - 0.2 * 32.0).abs() < 1e-3, "got {d}");
    }

    #[test]
    fn dtw_early_abort() {
        let a = line(0.0, 0.0, 1.0, 0.0);
        let b = line(0.0, 5.0, 1.0, 5.0);
        assert_eq!(dtw(&a, &b, 1.0), None);
    }

    #[test]
    fn hungarian_known_optimum() {
        // Optimal assignment: row0->col1 (1), row1->col0 (1) => total 2.
        let a = vec![vec![10.0, 1.0], vec![1.0, 10.0]];
        let assign = hungarian(&a);
        assert_eq!(assign, vec![1, 0]);
    }

    #[test]
    fn hungarian_matches_brute_force_random() {
        // Small random matrices: verify against exhaustive permutations.
        let mut seed = 12345u64;
        let mut rng = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..20 {
            let n = 4;
            let a: Vec<Vec<f64>> = (0..n)
                .map(|_| (0..n).map(|_| rng() as f64 / 100.0).collect())
                .collect();
            let assign = hungarian(&a);
            let mine: f64 = (0..n).map(|i| a[i][assign[i]]).sum();
            let mut best = f64::INFINITY;
            for p0 in 0..4 {
                for p1 in 0..4 {
                    for p2 in 0..4 {
                        for p3 in 0..4 {
                            let perm = [p0, p1, p2, p3];
                            if perm.iter().collect::<std::collections::HashSet<_>>().len() != 4 {
                                continue;
                            }
                            let s: f64 = (0..4).map(|i| a[i][perm[i]]).sum();
                            best = best.min(s);
                        }
                    }
                }
            }
            assert!((mine - best).abs() < 1e-9, "mine {mine} best {best}");
        }
    }

    #[test]
    fn recognize_real_dataset_top_candidate() {
        let db = crate::db::read_db(crate::db::DB_BYTES_FOR_TESTS).expect("dataset");
        // Draw 一 as a single horizontal stroke.
        let drawn: Vec<Vec<Pt>> = vec![vec![(0.1f32, 0.5f32), (0.5, 0.52), (0.9, 0.5)]];
        let cands = recognize(&drawn, &db, None, ScriptMask::ALL, 10);
        assert!(!cands.is_empty());
        assert_eq!(cands[0].ch, '一', "top candidate {:?}", &cands[..cands.len().min(5)]);
    }

    #[test]
    fn recognize_incremental_narrowing() {
        let db = crate::db::read_db(crate::db::DB_BYTES_FOR_TESTS).expect("dataset");
        // First stroke of 二: the short upper horizontal.
        let s1: Vec<Pt> = vec![(0.22f32, 0.30f32), (0.78, 0.31)];
        let pool = recognize(&[s1.clone()], &db, None, ScriptMask::ALL, 100);
        assert!(!pool.is_empty());
        // Second stroke: the longer lower horizontal.
        let s2: Vec<Pt> = vec![(0.11f32, 0.74f32), (0.89, 0.75)];
        let pool2 = recognize(&[s1, s2], &db, Some(&pool), ScriptMask::ALL, 100);
        assert!(!pool2.is_empty());
        assert_eq!(pool2[0].ch, '二', "top candidate {:?}", &pool2[..pool2.len().min(5)]);
    }

    /// Replay a character's own reference strokes as if drawn on the canvas.
    fn replay(db: &KanjiDb, ch: char) -> Vec<Vec<Pt>> {
        db.strokes_for(ch).expect("char in dataset").to_vec()
    }

    #[test]
    fn recognize_hiragana() {
        let db = crate::db::read_db(crate::db::DB_BYTES_FOR_TESTS).expect("dataset");
        for ch in ['あ', 'ひ', 'ず'] {
            let drawn = replay(&db, ch);
            let cands = recognize(&drawn, &db, None, ScriptMask::ALL, 10);
            assert_eq!(cands[0].ch, ch, "top candidate for {ch} {:?}", &cands[..cands.len().min(5)]);
            assert_eq!(cands[0].script(), Script::Hiragana);
        }
    }

    #[test]
    fn recognize_katakana() {
        let db = crate::db::read_db(crate::db::DB_BYTES_FOR_TESTS).expect("dataset");
        for ch in ['ア', 'ニ', 'シ'] {
            let drawn = replay(&db, ch);
            let cands = recognize(&drawn, &db, None, ScriptMask::ALL, 10);
            assert_eq!(cands[0].ch, ch, "top candidate for {ch} {:?}", &cands[..cands.len().min(5)]);
            assert_eq!(cands[0].script(), Script::Katakana);
        }
    }

    #[test]
    fn script_mask_filters_results() {
        let db = crate::db::read_db(crate::db::DB_BYTES_FOR_TESTS).expect("dataset");
        let drawn = replay(&db, 'あ');
        let kanji_only = ScriptMask { hiragana: false, katakana: false, kanji: true };
        let cands = recognize(&drawn, &db, None, kanji_only, 50);
        assert!(!cands.is_empty());
        assert!(cands.iter().all(|c| c.script() == Script::Kanji));
        assert!(!cands.iter().any(|c| c.ch == 'あ'));
    }

    #[test]
    fn alignment_variants_merge_close_pair() {
        // Two strokes whose shared endpoints nearly touch: one merge variant
        // joins them, plus the chained-run variant (same result here).
        let drawn = vec![
            vec![(0.1f32, 0.3), (0.5, 0.3)],
            vec![(0.52f32, 0.3), (0.9, 0.3)],
        ];
        let variants = alignment_variants(&drawn);
        assert_eq!(variants[0], drawn);
        assert!(
            variants.iter().any(|v| v.len() == 1 && v[0].len() == 4),
            "expected a merged variant: {variants:?}"
        );
    }

    #[test]
    fn alignment_variants_split_at_corner() {
        // A horizontal then vertical joined at a sharp corner (like the first
        // stroke of エ drawn as one): a variant splits it into two strokes.
        let drawn = vec![vec![(0.2f32, 0.2), (0.8, 0.2), (0.8, 0.8)]];
        let variants = alignment_variants(&drawn);
        assert!(
            variants.iter().any(|v| v.len() == 2),
            "expected a split variant: {variants:?}"
        );
    }

    #[test]
    fn alignment_variants_no_spurious_merge_or_split() {
        // Straight strokes with far-apart endpoints: only the identity reading.
        let drawn = vec![vec![(0.1f32, 0.3), (0.5, 0.3)], vec![(0.1, 0.7), (0.9, 0.7)]];
        let variants = alignment_variants(&drawn);
        assert_eq!(variants.len(), 1);
    }

    #[test]
    fn recognize_merged_strokes() {
        let db = crate::db::read_db(crate::db::DB_BYTES_FOR_TESTS).expect("dataset");
        // 一 drawn as two strokes with the pen lifted near the middle: the
        // shared endpoints nearly touch, so a merge variant must recover 一
        // even though the plain reading has the wrong stroke count.
        let s1: Vec<Pt> = vec![(0.15f32, 0.5), (0.5, 0.5)];
        let s2: Vec<Pt> = vec![(0.52f32, 0.5), (0.85, 0.5)];
        let cands = recognize(&[s1, s2], &db, None, ScriptMask::ALL, 10);
        assert!(!cands.is_empty());
        assert_eq!(
            cands[0].ch,
            '一',
            "top candidates {:?}",
            &cands[..cands.len().min(5)]
        );
    }
}
