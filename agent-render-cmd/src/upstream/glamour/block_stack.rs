//! Port of ansi/blockstack.go's Indent, Margin and Width calculations.
//! Upstream: cf874d7039af3485a38afa7d2e8e87ee42a7bbaa (MIT).
//! Adaptation: explicit Rust geometry values, saturating arithmetic, no Go buffers
//! or style inheritance. Prefix rendering is implemented by our Markdown adapter.

#[derive(Clone, Copy)]
pub(crate) struct Block {
    pub indent: usize,
    pub margin: usize,
}

#[derive(Default)]
pub(crate) struct BlockStack(Vec<Block>);

impl BlockStack {
    pub fn push(&mut self, block: Block) {
        self.0.push(block);
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }

    pub fn indent(&self) -> usize {
        self.0
            .iter()
            .fold(0usize, |sum, b| sum.saturating_add(b.indent))
    }

    pub fn margin(&self) -> usize {
        self.0
            .iter()
            .fold(0usize, |sum, b| sum.saturating_add(b.margin))
    }

    pub fn width(&self, total: usize) -> usize {
        total.saturating_sub(
            self.indent()
                .saturating_add(self.margin().saturating_mul(2)),
        )
    }
}
