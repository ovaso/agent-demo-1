//! Rendering of local Markdown events; block lifetime is owned by stream/state.
use super::{PrefixSpec, layout::Layout, link::Link, parse_options, supsub, table::Table};
use crate::ansi::Line;
use crate::{Options, ansi::Style, code::CodeBlocks, upstream::glamour::Theme};
use pulldown_cmark::{BrokenLink, CodeBlockKind, CowStr, Event, HeadingLevel, Parser, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn render_context(
    source: &str,
    options: &Options,
    codes: &mut CodeBlocks,
    prefixes: &[PrefixSpec],
    unresolved: &mut bool,
) -> Vec<Line> {
    codes.begin();
    let mut layout = Layout::new(options, Theme::DOCUMENT_MARGIN);
    for prefix in prefixes {
        layout.push_prefix(prefix.first.clone(), prefix.rest.clone(), prefix.indent);
    }
    let mut style = Style::PLAIN;
    let mut styles = Vec::new();
    let mut lists = Vec::new();
    let mut links = Vec::new();
    let mut depth = 0usize;
    let mut table: Option<Table<'_>> = None;
    let mut code: Option<(CowStr<'_>, String)> = None;
    let callback = |link: BrokenLink<'_>| {
        // Footnotes and our task/alert labels are not unresolved URL references.
        if !link.reference.starts_with(['^', '!'])
            && !matches!(link.reference.as_ref(), "✓" | " " | "")
        {
            *unresolved = true;
        }
        None
    };
    let events = Parser::new_with_broken_link_callback(source, parse_options(), Some(callback));
    for event in supsub::Events::new(events) {
        if let Some((_, text)) = code.as_mut() {
            if event == Event::End(TagEnd::CodeBlock) {
                let (info, text) = code.take().expect("active code block");
                let lines = codes.render(&info, &text, layout.available_width(), options);
                layout.block_lines(lines);
                depth = depth.saturating_sub(1);
                style = styles.pop().unwrap_or(Style::PLAIN);
            } else if let Event::Text(part) = event {
                text.push_str(&part);
            }
            continue;
        }
        if let Some(active) = table.as_mut() {
            if event == Event::End(TagEnd::Table) {
                let table = table.take().expect("active table");
                let lines = table.render(layout.available_width(), options.width_mode);
                layout.block_lines(lines);
                depth = depth.saturating_sub(1);
                style = styles.pop().unwrap_or(Style::PLAIN);
            } else {
                active.event(event);
            }
            continue;
        }
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    layout.before_block();
                }
                depth += 1;
                styles.push(style);
                match tag {
                    Tag::Heading { level, .. } => {
                        layout.finish_line();
                        style = if level == HeadingLevel::H1 {
                            Theme::H1
                        } else {
                            Theme::HEADING
                        };
                    }
                    Tag::Paragraph => layout.finish_line(),
                    Tag::FootnoteDefinition(label) => {
                        let prefix = format!("[^{label}]: ");
                        let width = prefix
                            .graphemes(true)
                            .map(|g| options.width_mode.width(g))
                            .sum();
                        layout.push_prefix(prefix, " ".repeat(width), width);
                    }
                    Tag::Strong => style.bold = true,
                    Tag::Emphasis => style.italic = true,
                    Tag::Strikethrough => style.strike = true,
                    Tag::BlockQuote(_) => layout.push_prefix(
                        Theme::QUOTE_PREFIX.into(),
                        Theme::QUOTE_PREFIX.into(),
                        2,
                    ),
                    Tag::List(start) => lists.push(start),
                    Tag::Item => {
                        let prefix = if let Some(Some(number)) = lists.last_mut() {
                            let prefix = format!("{number}. ");
                            *number = number.saturating_add(1);
                            prefix
                        } else {
                            Theme::ITEM_PREFIX.into()
                        };
                        let width = prefix.chars().count().max(Theme::LIST_INDENT);
                        layout.push_prefix(prefix, " ".repeat(width), width);
                    }
                    Tag::CodeBlock(kind) => {
                        let language = match kind {
                            CodeBlockKind::Fenced(info) => info,
                            CodeBlockKind::Indented => "".into(),
                        };
                        code = Some((language, String::new()));
                    }
                    Tag::Link { dest_url, .. } => {
                        links.push(Link::new(dest_url, false));
                        style = Theme::LINK_TEXT;
                    }
                    Tag::Image { dest_url, .. } => {
                        links.push(Link::new(dest_url, true));
                        style = Theme::LINK_TEXT;
                    }
                    Tag::Table(alignments) => table = Some(Table::new(alignments, options)),
                    _ => {}
                }
            }
            Event::End(tag) => {
                depth = depth.saturating_sub(1);
                style = styles.pop().unwrap_or(Style::PLAIN);
                match tag {
                    TagEnd::Paragraph | TagEnd::Heading(_) => layout.finish_line(),
                    TagEnd::Item | TagEnd::BlockQuote(_) | TagEnd::FootnoteDefinition => {
                        layout.pop_prefix()
                    }
                    TagEnd::List(_) => {
                        lists.pop();
                    }
                    TagEnd::Link | TagEnd::Image => {
                        if let Some(url) = links.pop().and_then(Link::destination) {
                            for link in &mut links {
                                link.observe(" (");
                                link.observe(&url);
                                link.observe(")");
                            }
                            layout.text(" (", style);
                            layout.text(&url, Theme::LINK);
                            layout.text(")", style);
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                for link in &mut links {
                    link.observe(&text);
                }
                layout.text(&text, style)
            }
            Event::Code(text) => {
                for link in &mut links {
                    link.observe(&text);
                }
                layout.text(&text, Theme::CODE);
            }
            Event::SoftBreak => {
                for link in &mut links {
                    link.observe(" ");
                }
                layout.text(" ", style);
            }
            Event::HardBreak => {
                for link in &mut links {
                    link.observe("\n");
                }
                layout.newline();
            }
            Event::Rule => {
                layout.before_block();
                layout.text(&"─".repeat(layout.available_width()), Theme::CODE_BLOCK);
                layout.finish_line();
            }
            Event::InlineMath(source) => {
                let text =
                    crate::math::inline(&source, options).unwrap_or_else(|| format!("${source}$"));
                for link in &mut links {
                    link.observe(&text);
                }
                layout.text(&text, style);
            }
            Event::DisplayMath(source) => {
                layout.before_block();
                if let Some(lines) =
                    crate::math::display(&source, layout.available_width(), options)
                {
                    layout.block_lines(lines);
                } else {
                    let text = crate::math::inline(source.trim(), options)
                        .unwrap_or_else(|| format!("$${source}$$"));
                    layout.text(&text, style);
                }
                layout.finish_line();
            }
            Event::TaskListMarker(checked) => {
                layout.text(if checked { "[✓] " } else { "[ ] " }, style)
            }
            Event::FootnoteReference(label) => layout.text(&format!("[^{label}]"), style),
        }
    }
    codes.end();
    layout.finish()
}
