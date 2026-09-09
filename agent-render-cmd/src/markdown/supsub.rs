//! Our inline HTML adapter. Unicode script characters are used only when every
//! visible character in the group has a mapping; otherwise retain ^{...}/_{...}.
//! Block HTML and literal code are deliberately not interpreted here.
use pulldown_cmark::{CowStr, Event, Tag, TagEnd};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Sup,
    Sub,
}
impl Position {
    fn tag(source: &str) -> Option<(Self, bool)> {
        let source = source.strip_prefix('<')?.strip_suffix('>')?;
        let (closing, source) = source
            .strip_prefix('/')
            .map_or((false, source), |s| (true, s));
        let end = source.find(char::is_whitespace).unwrap_or(source.len());
        let (name, rest) = source.split_at(end);
        if (closing && !rest.trim().is_empty()) || source.trim_end().ends_with('/') {
            return None;
        }
        let position = if name.eq_ignore_ascii_case("sup") {
            Self::Sup
        } else if name.eq_ignore_ascii_case("sub") {
            Self::Sub
        } else {
            return None;
        };
        Some((position, closing))
    }

    fn character(self, ch: char) -> Option<char> {
        // Use established modifier letters with exact Unicode compatibility
        // mappings. Missing letters remain missing, rather than changing case or
        // substituting similar-looking IPA characters. NUL is never emitted.
        const SUP_LOWER: [char; 26] = [
            'ᵃ', 'ᵇ', 'ᶜ', 'ᵈ', 'ᵉ', 'ᶠ', 'ᵍ', 'ʰ', 'ⁱ', 'ʲ', 'ᵏ', 'ˡ', 'ᵐ', 'ⁿ', 'ᵒ', 'ᵖ', '\0',
            'ʳ', 'ˢ', 'ᵗ', 'ᵘ', 'ᵛ', 'ʷ', 'ˣ', 'ʸ', 'ᶻ',
        ];
        const SUP_UPPER: [char; 26] = [
            'ᴬ', 'ᴮ', '\0', 'ᴰ', 'ᴱ', '\0', 'ᴳ', 'ᴴ', 'ᴵ', 'ᴶ', 'ᴷ', 'ᴸ', 'ᴹ', 'ᴺ', 'ᴼ', 'ᴾ', '\0',
            'ᴿ', '\0', 'ᵀ', 'ᵁ', 'ⱽ', 'ᵂ', '\0', '\0', '\0',
        ];
        const SUB_LOWER: [char; 26] = [
            'ₐ', '\0', '\0', '\0', 'ₑ', '\0', '\0', 'ₕ', 'ᵢ', 'ⱼ', 'ₖ', 'ₗ', 'ₘ', 'ₙ', 'ₒ', 'ₚ',
            '\0', 'ᵣ', 'ₛ', 'ₜ', 'ᵤ', 'ᵥ', '\0', 'ₓ', '\0', '\0',
        ];
        let mapped = match (self, ch) {
            (Self::Sup, 'a'..='z') => SUP_LOWER[ch as usize - 'a' as usize],
            (Self::Sup, 'A'..='Z') => SUP_UPPER[ch as usize - 'A' as usize],
            (Self::Sub, 'a'..='z') => SUB_LOWER[ch as usize - 'a' as usize],
            (Self::Sup, '0'..='9') => {
                ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'][ch as usize - '0' as usize]
            }
            (Self::Sub, '0'..='9') => {
                ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'][ch as usize - '0' as usize]
            }
            (Self::Sup, '+') => '⁺',
            (Self::Sub, '+') => '₊',
            (Self::Sup, '-' | '−') => '⁻',
            (Self::Sub, '-' | '−') => '₋',
            (Self::Sup, '=') => '⁼',
            (Self::Sub, '=') => '₌',
            (Self::Sup, '(') => '⁽',
            (Self::Sub, '(') => '₍',
            (Self::Sup, ')') => '⁾',
            (Self::Sub, ')') => '₎',
            (Self::Sub, 'ə') => 'ₔ',
            (_, ' ') => ' ',
            _ => '\0',
        };
        (mapped != '\0').then_some(mapped)
    }

    fn maps(self, event: &Event<'_>) -> bool {
        match event {
            Event::Text(text) => text.chars().all(|ch| self.character(ch).is_some()),
            Event::SoftBreak => true,
            Event::Start(Tag::Strong | Tag::Emphasis | Tag::Strikethrough)
            | Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => true,
            _ => false,
        }
    }
}

struct Frame<'a> {
    position: Position,
    opener: CowStr<'a>,
    events: Vec<Event<'a>>,
}

pub(super) struct Events<'a, I> {
    inner: I,
    stack: Vec<Frame<'a>>,
    ready: VecDeque<Event<'a>>,
}
impl<'a, I: Iterator<Item = Event<'a>>> Events<'a, I> {
    pub(super) fn new(inner: I) -> Self {
        Self {
            inner,
            stack: Vec::new(),
            ready: VecDeque::new(),
        }
    }

    fn emit(&mut self, event: Event<'a>) {
        if let Some(frame) = self.stack.last_mut() {
            frame.events.push(event);
        } else {
            self.ready.push_back(event);
        }
    }

    fn close(&mut self) {
        let frame = self.stack.pop().expect("matched opening tag");
        let mapped = frame.events.iter().all(|event| frame.position.maps(event));
        if !mapped {
            self.emit(Event::Text(
                match frame.position {
                    Position::Sup => "^{",
                    Position::Sub => "_{",
                }
                .into(),
            ));
        }
        for event in frame.events {
            self.emit(
                if let Event::Text(text) = &event
                    && mapped
                {
                    Event::Text(
                        text.chars()
                            .map(|ch| frame.position.character(ch).expect("validated mapping"))
                            .collect::<String>()
                            .into(),
                    )
                } else {
                    event
                },
            );
        }
        if !mapped {
            self.emit(Event::Text("}".into()));
        }
    }

    fn unfinished(&mut self) {
        // Only completed pairs transform. An unmatched opener stays literal and
        // cannot leak into another paragraph, list item or table cell.
        while let Some(frame) = self.stack.pop() {
            self.emit(Event::InlineHtml(frame.opener));
            for event in frame.events {
                self.emit(event);
            }
        }
    }
}

impl<'a, I: Iterator<Item = Event<'a>>> Iterator for Events<'a, I> {
    type Item = Event<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(event) = self.ready.pop_front() {
                return Some(event);
            }
            let Some(event) = self.inner.next() else {
                self.unfinished();
                return self.ready.pop_front();
            };
            if let Event::InlineHtml(html) = &event
                && let Some((position, closing)) = Position::tag(html)
            {
                if !closing {
                    let Event::InlineHtml(opener) = event else {
                        unreachable!()
                    };
                    self.stack.push(Frame {
                        position,
                        opener,
                        events: Vec::new(),
                    });
                    continue;
                }
                if self
                    .stack
                    .last()
                    .is_some_and(|frame| frame.position == position)
                {
                    self.close();
                    continue;
                }
            }
            if matches!(
                event,
                Event::End(
                    TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableCell
                )
            ) {
                self.unfinished();
            }
            if self.stack.is_empty() && self.ready.is_empty() {
                return Some(event);
            }
            self.emit(event);
        }
    }
}

#[cfg(feature = "math")]
pub(crate) fn script_text(text: &str, raised: bool) -> Option<String> {
    let position = if raised { Position::Sup } else { Position::Sub };
    text.chars().map(|ch| position.character(ch)).collect()
}
