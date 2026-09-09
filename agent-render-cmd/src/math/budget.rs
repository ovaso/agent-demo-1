//! Conservative layout bounds, checked before the upstream grid allocates cells.
use rust_latex_parser::EqNode as Node;
use unicode_width::UnicodeWidthStr;

pub(super) fn fits(node: &Node) -> bool {
    dimensions(node).is_some()
}
fn dimensions(node: &Node) -> Option<(usize, usize)> {
    let (w, h) = match node {
        Node::Text(text) | Node::TextBlock(text) => (text.width(), 1),
        Node::Space(_) => (2, 1),
        Node::Seq(nodes) => {
            let mut width = 0;
            let mut height = 1;
            for node in nodes {
                let (w, h) = dimensions(node)?;
                width += w;
                height = height.max(h);
            }
            (width, if height == 1 { 1 } else { height * 2 })
        }
        Node::Frac(a, b) | Node::Binom(a, b) => {
            let (aw, ah) = dimensions(a)?;
            let (bw, bh) = dimensions(b)?;
            (aw.max(bw) + 4, ah + bh + 1)
        }
        Node::Sup(a, b) | Node::Sub(a, b) => {
            let (aw, ah) = dimensions(a)?;
            let (bw, bh) = dimensions(b)?;
            (aw + bw + 1, ah + bh)
        }
        Node::SupSub(a, b, c) => {
            let (aw, ah) = dimensions(a)?;
            let (bw, bh) = dimensions(b)?;
            let (cw, ch) = dimensions(c)?;
            (aw + bw.max(cw) + 1, ah + bh + ch)
        }
        Node::Sqrt(body) | Node::Accent(body, _) => {
            let (w, h) = dimensions(body)?;
            (w + 2, h + 2)
        }
        Node::MathFont { content, .. } => dimensions(content)?,
        Node::Delimited { content, .. } => {
            let (w, h) = dimensions(content)?;
            (w + 4, h + 2)
        }
        Node::BigOp { lower, upper, .. } => {
            let mut width = 3;
            let mut height = 3;
            for n in [lower, upper].into_iter().flatten() {
                let (w, h) = dimensions(n)?;
                width = width.max(w);
                height += h;
            }
            (width, height)
        }
        Node::Matrix { rows, .. } => {
            let mut widths = Vec::new();
            let mut height = 0;
            for row in rows {
                let mut row_height = 1;
                for (i, n) in row.iter().enumerate() {
                    let (w, h) = dimensions(n)?;
                    if i == widths.len() {
                        widths.push(0)
                    }
                    widths[i] = widths[i].max(w);
                    row_height = row_height.max(h * 2);
                }
                height += row_height + 1;
            }
            (
                widths.iter().sum::<usize>() + widths.len() * 2 + 4,
                height + 2,
            )
        }
        Node::StackRel {
            base, annotation, ..
        } => {
            let (w, h) = dimensions(base)?;
            let (aw, ah) = dimensions(annotation)?;
            (w.max(aw), h + ah + 1)
        }
        _ => return None,
    };
    (w <= 512 && h <= 64 && w.saturating_mul(h) <= 8192).then_some((w, h))
}

pub(super) fn source(source: &str) -> bool {
    let mut chars = source.chars().peekable();
    let mut depth = 0usize;
    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                depth += 1;
                if depth > 32 {
                    return false;
                }
            }
            '}' => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            '%' => {
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        break;
                    }
                }
            }
            '\\' => {
                let mut command = String::new();
                while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                    command.push(chars.next().unwrap());
                }
                if command.is_empty() {
                    chars.next();
                }
                if matches!(
                    command.as_str(),
                    "def"
                        | "gdef"
                        | "edef"
                        | "xdef"
                        | "let"
                        | "futurelet"
                        | "newcommand"
                        | "renewcommand"
                        | "providecommand"
                        | "expandafter"
                        | "csname"
                ) {
                    return false;
                }
            }
            c if c.is_control() && !matches!(c, '\n' | '\r' | '\t') => return false,
            _ => {}
        }
    }
    depth == 0
}
