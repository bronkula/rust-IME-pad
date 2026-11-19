//! Minimal SVG path parser: supports M/L/H/V/C/S/Q/T/Z commands, absolute and relative.
//! kanjiVG stroke paths use mostly `M` + relative `c` curves.

#[derive(Debug, Clone, Copy)]
enum Token {
    Cmd(char),
    Num(f64),
}

fn tokenize(d: &str) -> Vec<Token> {
    let b = d.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            out.push(Token::Cmd(c));
            i += 1;
        } else if c == ',' || c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let start = i;
            // Optional sign at the start of the number only.
            if b[i] == b'-' || b[i] == b'+' {
                i += 1;
            }
            while i < b.len() {
                let ch = b[i] as char;
                if ch == 'e' || ch == 'E' {
                    i += 1;
                    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                        i += 1;
                    }
                } else if ch.is_ascii_digit() || ch == '.' {
                    i += 1;
                } else {
                    // A '-'/'+' here starts a new number (SVG implicit separator).
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse::<f64>() {
                out.push(Token::Num(v));
            }
        } else {
            i += 1;
        }
    }
    out
}

type P = (f64, f64);

fn cubic(p0: P, p1: P, p2: P, p3: P, steps: usize) -> Vec<P> {
    let mut out = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let u = 1.0 - t;
        let x = u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0;
        let y = u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1;
        out.push((x, y));
    }
    out
}

fn quad(p0: P, p1: P, p2: P, steps: usize) -> Vec<P> {
    let mut out = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let u = 1.0 - t;
        let x = u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0;
        let y = u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1;
        out.push((x, y));
    }
    out
}

/// Parse an SVG `d` attribute into a polyline of sampled points.
pub fn parse_path(d: &str) -> Vec<P> {
    let toks = tokenize(d);
    let mut pts: Vec<P> = Vec::new();
    let mut cur: P = (0.0, 0.0);
    let mut sub: P = (0.0, 0.0);
    let mut prev_ctrl: Option<P> = None;
    let mut i = 0;
    let mut cmd = 'M';

    fn num(toks: &[Token], i: &mut usize) -> Option<f64> {
        while *i < toks.len() {
            if let Token::Num(v) = toks[*i] {
                *i += 1;
                return Some(v);
            }
            *i += 1;
        }
        None
    }

    while i < toks.len() {
        if let Token::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
        }
        match cmd {
            'M' | 'm' => {
                let (Some(x), Some(y)) = (num(&toks, &mut i), num(&toks, &mut i)) else { break };
                cur = if cmd == 'm' { (cur.0 + x, cur.1 + y) } else { (x, y) };
                sub = cur;
                prev_ctrl = None;
                pts.push(cur);
                // Implicit repetition: coordinate pairs after M/m are treated as L/l.
                cmd = if cmd == 'M' { 'L' } else { 'l' };
            }
            'L' | 'l' | 'T' | 't' => {
                let (Some(x), Some(y)) = (num(&toks, &mut i), num(&toks, &mut i)) else { break };
                cur = if cmd.is_lowercase() { (cur.0 + x, cur.1 + y) } else { (x, y) };
                prev_ctrl = None;
                pts.push(cur);
            }
            'H' | 'h' => {
                let Some(x) = num(&toks, &mut i) else { break };
                cur = if cmd == 'h' { (cur.0 + x, cur.1) } else { (x, cur.1) };
                prev_ctrl = None;
                pts.push(cur);
            }
            'V' | 'v' => {
                let Some(y) = num(&toks, &mut i) else { break };
                cur = if cmd == 'v' { (cur.0, cur.1 + y) } else { (cur.0, y) };
                prev_ctrl = None;
                pts.push(cur);
            }
            'C' | 'c' => {
                let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let (p1x, p1y, p2x, p2y, p3x, p3y) = if cmd == 'c' {
                    (cur.0 + x1, cur.1 + y1, cur.0 + x2, cur.1 + y2, cur.0 + x, cur.1 + y)
                } else {
                    (x1, y1, x2, y2, x, y)
                };
                let seg = cubic(cur, (p1x, p1y), (p2x, p2y), (p3x, p3y), 24);
                // seg[0] duplicates cur; skip it
                pts.extend_from_slice(&seg[1..]);
                prev_ctrl = Some((p2x, p2y));
                cur = (p3x, p3y);
            }
            'S' | 's' => {
                let (Some(x2), Some(y2), Some(x), Some(y)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let p1 = match prev_ctrl {
                    Some(c) => (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1),
                    None => cur,
                };
                let (p2x, p2y, p3x, p3y) = if cmd == 's' {
                    (cur.0 + x2, cur.1 + y2, cur.0 + x, cur.1 + y)
                } else {
                    (x2, y2, x, y)
                };
                let seg = cubic(cur, p1, (p2x, p2y), (p3x, p3y), 24);
                pts.extend_from_slice(&seg[1..]);
                prev_ctrl = Some((p2x, p2y));
                cur = (p3x, p3y);
            }
            'Q' | 'q' => {
                let (Some(x1), Some(y1), Some(x), Some(y)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let (p1x, p1y, p2x, p2y) = if cmd == 'q' {
                    (cur.0 + x1, cur.1 + y1, cur.0 + x, cur.1 + y)
                } else {
                    (x1, y1, x, y)
                };
                let seg = quad(cur, (p1x, p1y), (p2x, p2y), 24);
                pts.extend_from_slice(&seg[1..]);
                prev_ctrl = Some((p1x, p1y));
                cur = (p2x, p2y);
            }
            'Z' | 'z' => {
                pts.push(sub);
                cur = sub;
                prev_ctrl = None;
            }
            _ => {
                i += 1;
            }
        }
    }

    if pts.is_empty() {
        pts.push(cur);
    }
    pts
}

/// Extract every `d="..."` attribute from an SVG document, in document order.
/// Only values that start with a moveto command (`M`/`m`) are kept, which
/// excludes kanjiVG group `id="kvg:..."` attributes (matched as substrings).
/// For kanjiVG files this yields exactly the stroke paths in stroke order.
pub fn extract_path_data(svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = svg[search_from..].find("d=\"") {
        let start = search_from + pos + 3;
        if let Some(end) = svg[start..].find('"') {
            let val = &svg[start..start + end];
            if val.starts_with('M') || val.starts_with('m') {
                out.push(val.to_string());
            }
            search_from = start + end + 1;
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_cubic_line() {
        // 一: horizontal-ish stroke
        let d = "M11,54.25c3.19,0.62,6.25,0.75,9.73,0.5c20.64-1.5,50.39-5.12,68.58-5.24c3.6-0.02,5.77,0.24,7.57,0.49";
        let pts = parse_path(d);
        assert!(pts.len() > 50);
        assert_eq!(pts[0], (11.0, 54.25));
        let last = pts[pts.len() - 1];
        assert!((last.0 - 96.88).abs() < 0.01, "end x {last:?}");
    }

    #[test]
    fn absolute_line_and_relative_line() {
        let pts = parse_path("M10,10 L20,10 l0,10");
        assert_eq!(pts, vec![(10.0, 10.0), (20.0, 10.0), (20.0, 20.0)]);
    }

    #[test]
    fn implicit_lineto_after_moveto() {
        let pts = parse_path("M10,10 20,20 30,30");
        assert_eq!(pts, vec![(10.0, 10.0), (20.0, 20.0), (30.0, 30.0)]);
    }

    #[test]
    fn close_path_returns_to_subpath_start() {
        let pts = parse_path("M10,10 L20,10 L20,20 Z");
        assert_eq!(pts, vec![(10.0, 10.0), (20.0, 10.0), (20.0, 20.0), (10.0, 10.0)]);
    }

    #[test]
    fn relative_moveto_sets_subpath() {
        let pts = parse_path("M10,10 m5,5 L20,20 Z");
        assert_eq!(pts, vec![(10.0, 10.0), (15.0, 15.0), (20.0, 20.0), (15.0, 15.0)]);
    }

    #[test]
    fn extract_paths_from_svg() {
        let svg = r#"<svg><g><path d="M1,2c3,4 5,6"/><path d="M7,8"/></g><g><text transform="matrix(1 0 0 1 4 5)">1</text></g></svg>"#;
        let ds = extract_path_data(svg);
        assert_eq!(ds, vec!["M1,2c3,4 5,6", "M7,8"]);
    }
}
