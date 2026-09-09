//! Shared link display policy for paragraphs and table cells (our implementation).

use pulldown_cmark::CowStr;

pub(super) struct Link<'a> {
    url: CowStr<'a>,
    matched_bytes: usize,
    label_matches: bool,
}

impl<'a> Link<'a> {
    pub fn new(url: CowStr<'a>, image: bool) -> Self {
        Self {
            url,
            matched_bytes: 0,
            label_matches: !image,
        }
    }

    /// Compare incrementally, so formatted labels need no extra text buffer.
    pub fn observe(&mut self, text: &str) {
        if !self.label_matches {
            return;
        }
        let target = self.url.strip_prefix("mailto:").unwrap_or(&self.url);
        if target
            .get(self.matched_bytes..)
            .is_some_and(|tail| tail.starts_with(text))
        {
            self.matched_bytes += text.len();
        } else {
            self.label_matches = false;
        }
    }

    pub fn destination(self) -> Option<CowStr<'a>> {
        let target = self.url.strip_prefix("mailto:").unwrap_or(&self.url);
        if self.url.is_empty() || (self.label_matches && self.matched_bytes == target.len()) {
            None
        } else {
            Some(self.url)
        }
    }
}
