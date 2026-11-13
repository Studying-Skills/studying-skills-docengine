//! A reader for the small part of SVG that formula renderers emit: groups with transforms, paths, rectangles and
//! `<use>` of earlier definitions, all filled. It yields plain outlines.
//!
//! A full SVG renderer weighs more than the rest of the engine together; formulas need none of it. Anything this
//! reader does not know (text, strokes, gradients, arcs) makes it give up, and the caller shows an empty frame
//! rather than a half-drawn formula.

use std::collections::HashMap;

use sfdoc_model::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Outlines of one colour, in the coordinates of the `viewBox`.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub color: Color,
    pub segs: Vec<Seg>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    /// `min-x, min-y, width, height` of the view box.
    pub view: [f32; 4],
    pub shapes: Vec<Shape>,
}

/// Row-major 2×3 affine: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Clone, Copy)]
struct M([f32; 6]);

impl M {
    const ID: M = M([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    fn then(self, o: M) -> M {
        // self applied after o (o is the inner transform).
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = o.0;
        M([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ])
    }
    fn at(self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

fn numbers(s: &str) -> Option<Vec<f32>> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = *b.get(i)?;
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        let start = i;
        if c == b'-' || c == b'+' {
            i += 1;
        }
        let mut dot = false;
        while let Some(&c) = b.get(i) {
            if c.is_ascii_digit() {
                i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                i += 1;
            } else {
                break;
            }
        }
        if matches!(b.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(b.get(j), Some(b'-' | b'+')) {
                j += 1;
            }
            if b.get(j).is_some_and(u8::is_ascii_digit) {
                while b.get(j).is_some_and(u8::is_ascii_digit) {
                    j += 1;
                }
                i = j;
            }
        }
        out.push(s.get(start..i)?.parse().ok()?);
    }
    Some(out)
}

fn transform(s: &str) -> Option<M> {
    let mut m = M::ID;
    let mut rest = s.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let close = rest.find(')')?;
        let name = rest.get(..open)?.trim().trim_start_matches(',').trim();
        let v = numbers(rest.get(open + 1..close)?)?;
        let g = |i: usize| v.get(i).copied();
        let t = match name {
            "matrix" => M([g(0)?, g(1)?, g(2)?, g(3)?, g(4)?, g(5)?]),
            "translate" => M([1.0, 0.0, 0.0, 1.0, g(0)?, g(1).unwrap_or(0.0)]),
            "scale" => M([g(0)?, 0.0, 0.0, g(1).or(g(0))?, 0.0, 0.0]),
            "rotate" if v.len() == 1 => {
                let (sin, cos) = g(0)?.to_radians().sin_cos();
                M([cos, sin, -sin, cos, 0.0, 0.0])
            }
            _ => return None,
        };
        m = m.then(t);
        rest = rest.get(close + 1..)?.trim();
    }
    Some(m)
}

/// Path data → absolute segments, transformed by `m`. `None` on anything outside the supported commands.
fn path(d: &str, m: M, out: &mut Vec<Seg>) -> Option<()> {
    let b = d.as_bytes();
    let (mut i, mut cmd) = (0usize, b' ');
    let (mut x, mut y, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    // The previous control point, for the smooth commands.
    let mut prev: Option<(u8, f32, f32)> = None;
    loop {
        while b.get(i).is_some_and(|c| c.is_ascii_whitespace() || *c == b',') {
            i += 1;
        }
        let Some(&c) = b.get(i) else { break };
        if c.is_ascii_alphabetic() {
            cmd = c;
            i += 1;
            if matches!(cmd, b'Z' | b'z') {
                out.push(Seg::Close);
                (x, y) = (sx, sy);
                prev = None;
                continue;
            }
        } else if cmd == b' ' {
            return None;
        }
        let count = match cmd.to_ascii_uppercase() {
            b'M' | b'L' | b'T' => 2,
            b'H' | b'V' => 1,
            b'C' => 6,
            b'S' | b'Q' => 4,
            _ => return None,
        };
        // Read exactly `count` numbers.
        let start = i;
        let mut got = 0;
        let mut end = i;
        while got < count {
            while b.get(end).is_some_and(|c| c.is_ascii_whitespace() || *c == b',') {
                end += 1;
            }
            let from = end;
            if matches!(b.get(end), Some(b'-' | b'+')) {
                end += 1;
            }
            let mut dot = false;
            while let Some(&c) = b.get(end) {
                if c.is_ascii_digit() || (c == b'.' && !dot) {
                    dot |= c == b'.';
                    end += 1;
                } else {
                    break;
                }
            }
            if matches!(b.get(end), Some(b'e' | b'E'))
                && b.get(end + 1).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+'))
            {
                end += 2;
                while b.get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
            }
            if end == from {
                return None;
            }
            got += 1;
        }
        let v = numbers(d.get(start..end)?)?;
        i = end;
        let rel = cmd.is_ascii_lowercase();
        let n = |k: usize| v.get(k).copied();
        let px = |k: usize| n(k).map(|val| if rel { x + val } else { val });
        let py = |k: usize| n(k).map(|val| if rel { y + val } else { val });
        match cmd.to_ascii_uppercase() {
            b'M' => {
                (x, y) = (px(0)?, py(1)?);
                (sx, sy) = (x, y);
                let p = m.at(x, y);
                out.push(Seg::Move(p.0, p.1));
                // Further pairs after a moveto are linetos.
                cmd = if rel { b'l' } else { b'L' };
                prev = None;
            }
            b'L' => {
                (x, y) = (px(0)?, py(1)?);
                let p = m.at(x, y);
                out.push(Seg::Line(p.0, p.1));
                prev = None;
            }
            b'H' => {
                x = px(0)?;
                let p = m.at(x, y);
                out.push(Seg::Line(p.0, p.1));
                prev = None;
            }
            b'V' => {
                y = py(0)?;
                let p = m.at(x, y);
                out.push(Seg::Line(p.0, p.1));
                prev = None;
            }
            b'C' | b'S' => {
                let (c1, c2, to) = if cmd.eq_ignore_ascii_case(&b'C') {
                    ((px(0)?, py(1)?), (px(2)?, py(3)?), (px(4)?, py(5)?))
                } else {
                    let c1 = match prev {
                        Some((b'C', cx, cy)) => (2.0 * x - cx, 2.0 * y - cy),
                        _ => (x, y),
                    };
                    (c1, (px(0)?, py(1)?), (px(2)?, py(3)?))
                };
                let (a, b2, c3) = (m.at(c1.0, c1.1), m.at(c2.0, c2.1), m.at(to.0, to.1));
                out.push(Seg::Cubic(a.0, a.1, b2.0, b2.1, c3.0, c3.1));
                prev = Some((b'C', c2.0, c2.1));
                (x, y) = to;
            }
            b'Q' | b'T' => {
                let (c1, to) = if cmd.eq_ignore_ascii_case(&b'Q') {
                    ((px(0)?, py(1)?), (px(2)?, py(3)?))
                } else {
                    let c1 = match prev {
                        Some((b'Q', cx, cy)) => (2.0 * x - cx, 2.0 * y - cy),
                        _ => (x, y),
                    };
                    (c1, (px(0)?, py(1)?))
                };
                let (a, t) = (m.at(c1.0, c1.1), m.at(to.0, to.1));
                out.push(Seg::Quad(a.0, a.1, t.0, t.1));
                prev = Some((b'Q', c1.0, c1.1));
                (x, y) = to;
            }
            _ => return None,
        }
    }
    Some(())
}

// ---- a very small XML reader: elements and attributes, no entities beyond the five predefined ----

struct El<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, String)>,
    children: Vec<El<'a>>,
}

impl El<'_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Parses one element starting at `s[*i] == '<'`. Text content is skipped, except that non-blank text inside a
/// `<text>` or `<title>`-like element is the caller's concern (elements are reported by name).
fn element<'a>(s: &'a str, i: &mut usize, depth: u32) -> Option<El<'a>> {
    if depth > 64 {
        return None;
    }
    let b = s.as_bytes();
    if *b.get(*i)? != b'<' {
        return None;
    }
    *i += 1;
    let start = *i;
    while b.get(*i).is_some_and(|c| !c.is_ascii_whitespace() && *c != b'>' && *c != b'/') {
        *i += 1;
    }
    let name = s.get(start..*i)?;
    let mut attrs = Vec::new();
    loop {
        while b.get(*i).is_some_and(u8::is_ascii_whitespace) {
            *i += 1;
        }
        match *b.get(*i)? {
            b'/' => {
                *i += 1;
                if *b.get(*i)? != b'>' {
                    return None;
                }
                *i += 1;
                return Some(El { name, attrs, children: Vec::new() });
            }
            b'>' => {
                *i += 1;
                break;
            }
            _ => {
                let ks = *i;
                while b.get(*i).is_some_and(|c| !c.is_ascii_whitespace() && *c != b'=') {
                    *i += 1;
                }
                let key = s.get(ks..*i)?;
                while b.get(*i).is_some_and(u8::is_ascii_whitespace) {
                    *i += 1;
                }
                if *b.get(*i)? != b'=' {
                    return None;
                }
                *i += 1;
                while b.get(*i).is_some_and(u8::is_ascii_whitespace) {
                    *i += 1;
                }
                let q = *b.get(*i)?;
                if q != b'"' && q != b'\'' {
                    return None;
                }
                *i += 1;
                let vs = *i;
                while *b.get(*i)? != q {
                    *i += 1;
                }
                attrs.push((key, unescape(s.get(vs..*i)?)));
                *i += 1;
            }
        }
    }
    let mut children = Vec::new();
    loop {
        // Skip text up to the next tag.
        while *b.get(*i)? != b'<' {
            *i += 1;
        }
        let rest = s.get(*i..)?;
        if rest.starts_with("</") {
            let close = rest.find('>')?;
            if rest.get(2..close)?.trim() != name {
                return None;
            }
            *i += close + 1;
            return Some(El { name, attrs, children });
        }
        if rest.starts_with("<!--") {
            *i += rest.find("-->")? + 3;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            *i += rest.find("]]>")? + 3;
            continue;
        }
        children.push(element(s, i, depth + 1)?);
    }
}

fn root(s: &str) -> Option<El<'_>> {
    let mut i = 0;
    loop {
        let rest = s.get(i..)?;
        let at = rest.find('<')?;
        i += at;
        let rest = s.get(i..)?;
        if rest.starts_with("<?") {
            i += rest.find("?>")? + 2;
        } else if rest.starts_with("<!--") {
            i += rest.find("-->")? + 3;
        } else if rest.starts_with("<!") {
            i += rest.find('>')? + 1;
        } else {
            return element(s, &mut i, 0);
        }
    }
}

fn color(s: &str, inherited: Color) -> Option<Option<Color>> {
    Some(match s.trim() {
        "none" => None,
        "currentColor" | "inherit" => Some(inherited),
        "black" => Some(Color::BLACK),
        "white" => Some(Color::WHITE),
        "red" => Some(Color::rgb(255, 0, 0)),
        "green" => Some(Color::rgb(0, 128, 0)),
        "blue" => Some(Color::rgb(0, 0, 255)),
        other => Some(Color::parse(other)?),
    })
}

struct Walk<'a, 'e> {
    ids: HashMap<&'e str, &'e El<'a>>,
    shapes: Vec<Shape>,
    budget: u32,
}

impl<'a, 'e> Walk<'a, 'e> {
    fn index(&mut self, el: &'e El<'a>) {
        if let Some(id) = el.attr("id") {
            self.ids.insert(id, el);
        }
        for c in &el.children {
            self.index(c);
        }
    }

    fn push(&mut self, fill: Option<Color>, segs: Vec<Seg>) {
        let Some(color) = fill else { return };
        if segs.is_empty() {
            return;
        }
        match self.shapes.last_mut() {
            Some(last) if last.color == color => last.segs.extend(segs),
            _ => self.shapes.push(Shape { color, segs }),
        }
    }

    fn draw(&mut self, el: &'e El<'a>, m: M, fill: Option<Color>, depth: u32) -> Option<()> {
        // `<use>` can nest and repeat: bound the work a hostile file can ask for.
        self.budget = self.budget.checked_sub(1)?;
        if depth > 64 {
            return None;
        }
        let m = match el.attr("transform") {
            Some(t) => m.then(transform(t)?),
            None => m,
        };
        let fill = match el.attr("fill") {
            Some(f) => color(f, fill.unwrap_or(Color::BLACK))?,
            None => fill,
        };
        // A stroke that is actually drawn is outside what this reader can do.
        let stroked = el.attr("stroke").is_some_and(|s| s != "none")
            && el.attr("stroke-width").is_none_or(|w| w.trim().parse::<f32>().is_ok_and(|w| w != 0.0));
        let num = |name: &str| el.attr(name).map_or(Some(0.0), |v| v.trim().trim_end_matches("px").parse::<f32>().ok());
        match el.name {
            "g" | "a" => {
                for c in &el.children {
                    self.draw(c, m, fill, depth + 1)?;
                }
            }
            "svg" => {
                // A nested viewport: MathJax uses it for stretched delimiters.
                let (x, y) = (num("x")?, num("y")?);
                let mut inner = m.then(M([1.0, 0.0, 0.0, 1.0, x, y]));
                if let (Some(vb), Some(w), Some(h)) = (el.attr("viewBox"), el.attr("width"), el.attr("height")) {
                    let v = numbers(vb)?;
                    let (vx, vy, vw, vh) = (*v.first()?, *v.get(1)?, *v.get(2)?, *v.get(3)?);
                    let (w, h) = (w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?);
                    if vw <= 0.0 || vh <= 0.0 {
                        return None;
                    }
                    // preserveAspectRatio="none" is what stretching means; the default (uniform, centred) is not
                    // told apart here because MathJax always says "none" on these.
                    inner = inner.then(M([w / vw, 0.0, 0.0, h / vh, -vx * w / vw, -vy * h / vh]));
                }
                for c in &el.children {
                    self.draw(c, inner, fill, depth + 1)?;
                }
            }
            "path" => {
                if stroked {
                    return None;
                }
                let mut segs = Vec::new();
                path(el.attr("d").unwrap_or(""), m, &mut segs)?;
                self.push(fill, segs);
            }
            "rect" => {
                if stroked {
                    return None;
                }
                let (x, y, w, h) = (num("x")?, num("y")?, num("width")?, num("height")?);
                let p = [m.at(x, y), m.at(x + w, y), m.at(x + w, y + h), m.at(x, y + h)];
                let segs = vec![
                    Seg::Move(p[0].0, p[0].1),
                    Seg::Line(p[1].0, p[1].1),
                    Seg::Line(p[2].0, p[2].1),
                    Seg::Line(p[3].0, p[3].1),
                    Seg::Close,
                ];
                self.push(fill, segs);
            }
            "use" => {
                let href = el.attr("xlink:href").or_else(|| el.attr("href"))?;
                let target = *self.ids.get(href.strip_prefix('#')?)?;
                let m = m.then(M([1.0, 0.0, 0.0, 1.0, num("x")?, num("y")?]));
                self.draw(target, m, fill, depth + 1)?;
            }
            "defs" | "title" | "desc" | "metadata" | "style" => {}
            _ => return None,
        }
        Some(())
    }
}

/// Read an SVG made of filled outlines. `None` when the file uses anything else.
pub fn read(svg: &str) -> Option<Drawing> {
    // Formulas are a few kilobytes; refuse to chew on something huge.
    if svg.len() > 4_000_000 {
        return None;
    }
    let root = root(svg)?;
    if root.name != "svg" {
        return None;
    }
    let v = numbers(root.attr("viewBox")?)?;
    let view = [*v.first()?, *v.get(1)?, *v.get(2)?, *v.get(3)?];
    if !(view[2] > 0.0 && view[3] > 0.0) {
        return None;
    }
    let mut w = Walk { ids: HashMap::new(), shapes: Vec::new(), budget: 200_000 };
    w.index(&root);
    let fill = match root.attr("fill") {
        Some(f) => color(f, Color::BLACK)?,
        None => Some(Color::BLACK),
    };
    for c in &root.children {
        w.draw(c, M::ID, fill, 0)?;
    }
    Some(Drawing { view, shapes: w.shapes })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn paths_groups_and_rects_become_absolute_outlines() {
        let d = read(
            r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -700 1000 900">
              <defs><path id="a" d="M0 0h10v10z"/></defs>
              <g stroke="currentColor" fill="currentColor" stroke-width="0" transform="scale(1,-1)">
                <g transform="translate(100,50)"><use xlink:href="#a"/><path d="m1 1 2 2Q5 5 6 3T8 3C9 3 9 4 9 5S8 7 7 7Z"/></g>
                <rect x="0" y="220" width="500" height="60"/>
                <g fill="#ff0000"><path d="M0,0L1,1"/></g>
              </g></svg>"##,
        )
        .unwrap();
        assert_eq!(d.view, [0.0, -700.0, 1000.0, 900.0]);
        assert_eq!(d.shapes.len(), 2);
        let s = &d.shapes[0].segs;
        // The <use>d triangle, moved by the group and flipped by the outer scale.
        assert_eq!(s[..4], [Seg::Move(100.0, -50.0), Seg::Line(110.0, -50.0), Seg::Line(110.0, -60.0), Seg::Close]);
        // Relative moveto then implicit lineto; T mirrors the Q control point; S mirrors the C one.
        assert_eq!(s[4], Seg::Move(101.0, -51.0));
        assert_eq!(s[5], Seg::Line(103.0, -53.0));
        assert_eq!(s[6], Seg::Quad(105.0, -55.0, 106.0, -53.0));
        assert_eq!(s[7], Seg::Quad(107.0, -51.0, 108.0, -53.0));
        assert_eq!(s[9], Seg::Cubic(109.0, -56.0, 108.0, -57.0, 107.0, -57.0));
        assert_eq!(s[11], Seg::Move(0.0, -220.0));
        assert_eq!(d.shapes[1].color, Color::rgb(255, 0, 0));
    }

    #[test]
    fn anything_unknown_is_refused_whole() {
        let wrap = |inner: &str| format!(r#"<svg viewBox="0 0 10 10">{inner}</svg>"#);
        assert!(read(&wrap(r#"<path d="M0 0L1 1"/>"#)).is_some());
        for bad in [
            r#"<text x="1" y="1">а</text>"#,
            r#"<path d="M0 0A5 5 0 0 1 3 3"/>"#,
            r#"<path d="M0 0L1 1" stroke="black" stroke-width="2"/>"#,
            r#"<path d="M0 0L1"/>"#,
            r#"<path d="L"/>"#,
            r##"<use href="#nope"/>"##,
            r#"<g transform="skewX(3)"/>"#,
            r#"<circle r="3"/>"#,
            r#"<g fill="url(#grad)"><path d="M0 0L1 1"/></g>"#,
        ] {
            assert!(read(&wrap(bad)).is_none(), "{bad}");
        }
        assert!(read("<svg>").is_none());
        assert!(read(r#"<svg viewBox="0 0 0 10"/>"#).is_none());
        assert!(read("not xml").is_none());
        assert!(read(r#"<html viewBox="0 0 1 1"/>"#).is_none());
        // A <use> cycle ends by the work budget instead of the stack.
        assert!(read(r##"<svg viewBox="0 0 1 1"><g id="a"><use href="#a"/><use href="#a"/></g></svg>"##).is_none());
    }

    #[test]
    fn garbage_never_panics() {
        let seed = r##"<svg viewBox="0 0 10 10"><g transform="translate(1,2) scale(3)"><path d="M0 0h1e2v-.5.5z"/><use href="#x" x="1"/></g><!-- c --></svg>"##;
        for cut in 0..seed.len() {
            if seed.is_char_boundary(cut) {
                let _ = read(&seed[..cut]);
                let _ = read(&seed[cut..]);
            }
        }
    }
}
