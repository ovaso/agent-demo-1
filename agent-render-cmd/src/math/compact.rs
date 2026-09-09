//! Unambiguous single-line notation for prose, tables and narrow terminals.
use rust_latex_parser::EqNode as Node;

pub(super) fn render(node: &Node) -> Option<String> {
    Some(match node {
        Node::Text(t) | Node::TextBlock(t) => t.clone(),
        Node::Space(p) => if *p > 0.0 { " " } else { "" }.into(),
        Node::Seq(nodes) => {
            let mut text = String::new();
            for n in nodes {
                text.push_str(&render(n)?);
            }
            text
        }
        Node::Frac(a, b) => format!("({})/({})", render(a)?.trim(), render(b)?.trim()),
        Node::Sqrt(n) => format!("√({})", render(n)?.trim()),
        Node::Sup(a, b) => format!("{}{}", render(a)?, script(&tight(b)?, true)),
        Node::Sub(a, b) => format!("{}{}", render(a)?, script(&tight(b)?, false)),
        Node::SupSub(a, b, c) => format!(
            "{}{}{}",
            render(a)?,
            script(&tight(c)?, false),
            script(&tight(b)?, true)
        ),
        Node::BigOp {
            symbol,
            lower,
            upper,
        } => {
            let mut text = symbol.clone();
            if let Some(n) = lower {
                text.push_str(&script(&tight(n)?, false));
            }
            if let Some(n) = upper {
                text.push_str(&script(&tight(n)?, true));
            }
            text
        }
        Node::Delimited {
            left,
            right,
            content,
        } => format!("{left}{}{right}", render(content)?),
        Node::MathFont { kind, content } => {
            let block = term_maths::layout::layout(&Node::MathFont {
                kind: *kind,
                content: Box::new(Node::Text(render(content)?)),
            });
            if block.height() != 1 {
                return None;
            }
            block.to_string()
        }
        Node::Matrix { rows, .. } => {
            let mut result = String::from("[");
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    result.push_str("; ");
                }
                for (j, n) in row.iter().enumerate() {
                    if j > 0 {
                        result.push_str(", ");
                    }
                    result.push_str(render(n)?.trim());
                }
            }
            result.push(']');
            result
        }
        _ => return None,
    })
}
fn tight(node: &Node) -> Option<String> {
    // Generated operator spacing is unnecessary in a compact script. Literal
    // text blocks still retain their internal spaces and word boundaries.
    match node {
        Node::Text(text) => Some(text.trim().to_owned()),
        Node::Seq(nodes) => {
            let mut text = String::new();
            for node in nodes {
                text.push_str(&tight(node)?);
            }
            Some(text)
        }
        _ => render(node),
    }
}
fn script(text: &str, raised: bool) -> String {
    let text = text.trim();
    crate::markdown::script_text(text, raised)
        .unwrap_or_else(|| format!("{}{{{text}}}", if raised { '^' } else { '_' }))
}
