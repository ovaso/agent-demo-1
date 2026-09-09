//! Strict LaTeX events -> term-maths' public AST. No upstream parser code is copied.
use pulldown_latex::{Parser, Storage, event::*};
use rust_latex_parser::{EqNode as Node, MathFontKind, MatrixKind};
use std::collections::VecDeque;

pub(super) fn parse(source: &str, display: bool) -> Option<Node> {
    if source.len() > 4096 || !super::budget::source(source) {
        return None;
    }
    let storage = Storage::new();
    let events = Parser::new(source, &storage)
        .take(1025)
        .collect::<std::result::Result<VecDeque<_>, _>>()
        .ok()?;
    if events.len() > 1024 {
        return None;
    }
    let mut parser = Nodes { events, depth: 0 };
    let node = parser
        .sequence(Context {
            font: None,
            display,
        })
        .ok()?;
    parser.events.is_empty().then_some(node)
}

#[derive(Clone, Copy)]
struct Context {
    font: Option<MathFontKind>,
    display: bool,
}
struct Nodes<'a> {
    events: VecDeque<Event<'a>>,
    depth: usize,
}
type Result<T> = std::result::Result<T, ()>;

pub(super) fn sequence(mut nodes: Vec<Node>) -> Node {
    if nodes.len() == 1 {
        nodes.pop().unwrap()
    } else {
        Node::Seq(nodes)
    }
}
impl Nodes<'_> {
    fn sequence(&mut self, mut context: Context) -> Result<Node> {
        let mut nodes = Vec::new();
        loop {
            match self.events.front() {
                None | Some(Event::End | Event::EnvironmentFlow(_)) => break,
                Some(Event::StateChange(_)) => match self.events.pop_front().unwrap() {
                    Event::StateChange(StateChange::Font(font)) => {
                        context.font = font.map(font_kind).transpose()?
                    }
                    Event::StateChange(StateChange::Style(style)) => {
                        context.display = style == Style::Display
                    }
                    _ => return Err(()),
                },
                _ => nodes.push(self.element(context)?),
            }
        }
        Ok(sequence(nodes))
    }
    fn element(&mut self, context: Context) -> Result<Node> {
        self.depth += 1;
        if self.depth > 32 {
            return Err(());
        }
        let result = self.element_inner(context);
        self.depth -= 1;
        result
    }
    fn element_inner(&mut self, context: Context) -> Result<Node> {
        let node = match self.events.pop_front().ok_or(())? {
            Event::Content(content) => {
                let text = match content {
                    Content::Text(text) => return Ok(Node::TextBlock(text.into())),
                    Content::Number(text) | Content::Function(text) => text.into(),
                    Content::Ordinary { content, .. }
                    | Content::Delimiter { content, .. }
                    | Content::Punctuation(content) => content.to_string(),
                    Content::BinaryOp { content, .. } => format!(" {content} "),
                    Content::Relation { content, .. } => format!(
                        " {} ",
                        std::str::from_utf8(content.encode_utf8_to_buf(&mut [0; 8]))
                            .map_err(|_| ())?
                    ),
                    Content::LargeOp { content, .. } => {
                        return Ok(Node::BigOp {
                            symbol: content.to_string(),
                            lower: None,
                            upper: None,
                        });
                    }
                };
                if let Some(kind) = context.font {
                    Node::MathFont {
                        kind,
                        content: Box::new(Node::Text(text)),
                    }
                } else {
                    Node::Text(text)
                }
            }
            Event::Begin(group) => {
                let node = match group {
                    Grouping::Normal => self.sequence(context)?,
                    Grouping::LeftRight(left, right) => Node::Delimited {
                        left: left.map_or(String::new(), |c| c.to_string()),
                        right: right.map_or(String::new(), |c| c.to_string()),
                        content: Box::new(self.sequence(context)?),
                    },
                    Grouping::Matrix { .. }
                    | Grouping::Aligned
                    | Grouping::Gathered
                    | Grouping::Split
                    | Grouping::Align { eq_numbers: false } => self.matrix(context)?,
                    Grouping::Cases { left } => Node::Delimited {
                        left: if left { "{" } else { "" }.into(),
                        right: if left { "" } else { "}" }.into(),
                        content: Box::new(self.matrix(context)?),
                    },
                    _ => return Err(()),
                };
                if !matches!(self.events.pop_front(), Some(Event::End)) {
                    return Err(());
                }
                node
            }
            Event::Visual(Visual::SquareRoot) => Node::Sqrt(Box::new(self.element(context)?)),
            Event::Visual(Visual::Root) => {
                let body = self.element(context)?;
                let index = self.element(context)?;
                sequence(vec![
                    Node::Sup(Box::new(Node::Text(String::new())), Box::new(index)),
                    Node::Sqrt(Box::new(body)),
                ])
            }
            Event::Visual(Visual::Fraction(thickness)) => {
                let numerator = Box::new(self.element(context)?);
                let denominator = Box::new(self.element(context)?);
                if thickness.is_some_and(|d| d.value == 0.0) {
                    Node::StackRel {
                        base: denominator,
                        annotation: numerator,
                        over: true,
                    }
                } else {
                    Node::Frac(numerator, denominator)
                }
            }
            Event::Visual(Visual::Negation) => {
                let Node::Text(text) = self.element(context)? else {
                    return Err(());
                };
                let text = text.trim();
                if text.chars().count() != 1 {
                    return Err(());
                }
                Node::Text(format!("{text}\u{338}"))
            }
            Event::Script { ty, position } => {
                let base = self.element(context)?;
                let lower = if matches!(ty, ScriptType::Subscript | ScriptType::SubSuperscript) {
                    Some(Box::new(self.element(context)?))
                } else {
                    None
                };
                let upper = if matches!(ty, ScriptType::Superscript | ScriptType::SubSuperscript) {
                    Some(Box::new(self.element(context)?))
                } else {
                    None
                };
                let vertical = position == ScriptPosition::AboveBelow
                    || (position == ScriptPosition::Movable && context.display);
                scripts(base, lower, upper, vertical)
            }
            Event::Space {
                width,
                height: None,
                depth: None,
            } => Node::Space(width.map_or(0.0, |d| if d.value > 0.0 { 4.0 } else { 0.0 })),
            _ => return Err(()),
        };
        Ok(node)
    }
    fn matrix(&mut self, context: Context) -> Result<Node> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        loop {
            row.push(self.sequence(context)?);
            match self.events.front() {
                Some(Event::End) => {
                    rows.push(row);
                    break;
                }
                Some(Event::EnvironmentFlow(EnvironmentFlow::Alignment)) => {
                    self.events.pop_front();
                }
                Some(Event::EnvironmentFlow(EnvironmentFlow::NewLine {
                    spacing: None,
                    horizontal_lines,
                })) if horizontal_lines.is_empty() => {
                    self.events.pop_front();
                    rows.push(std::mem::take(&mut row));
                    if matches!(self.events.front(), Some(Event::End)) {
                        break;
                    }
                }
                _ => return Err(()),
            }
            if row.len() > 16 || rows.len() > 16 {
                return Err(());
            }
        }
        Ok(Node::Matrix {
            kind: MatrixKind::Plain,
            rows,
        })
    }
}

fn scripts(base: Node, lower: Option<Box<Node>>, upper: Option<Box<Node>>, vertical: bool) -> Node {
    if vertical {
        if let Node::BigOp { symbol, .. } = base {
            return Node::BigOp {
                symbol,
                lower,
                upper,
            };
        }
        let mut node = base;
        if let Some(annotation) = upper {
            node = Node::StackRel {
                base: Box::new(node),
                annotation,
                over: true,
            };
        }
        if let Some(annotation) = lower {
            node = Node::StackRel {
                base: Box::new(node),
                annotation,
                over: false,
            };
        }
        return node;
    }
    match (lower, upper) {
        (Some(sub), Some(sup)) => Node::SupSub(Box::new(base), sup, sub),
        (Some(sub), None) => Node::Sub(Box::new(base), sub),
        (None, Some(sup)) => Node::Sup(Box::new(base), sup),
        _ => base,
    }
}
fn font_kind(font: Font) -> Result<MathFontKind> {
    Ok(match font {
        Font::Bold => MathFontKind::Bold,
        Font::DoubleStruck => MathFontKind::Blackboard,
        Font::Script => MathFontKind::Calligraphic,
        Font::Fraktur => MathFontKind::Fraktur,
        Font::SansSerif => MathFontKind::SansSerif,
        Font::Monospace => MathFontKind::Monospace,
        Font::UpRight => MathFontKind::Roman,
        _ => return Err(()),
    })
}
