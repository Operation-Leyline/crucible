//! Byte ranges into one source file.

/// A half-open byte range `start..end` into the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        // Model files are far below 4 GiB; a larger one is refused by `parse`.
        Self {
            start: start as u32,
            end: end as u32,
        }
    }

    /// The span covering both `self` and `other`.
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn text(self, source: &str) -> &str {
        &source[self.start as usize..self.end as usize]
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// Maps byte offsets to 1-based line and column numbers.
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index as u32 + 1),
        );
        Self { starts }
    }

    /// The 1-based line and column (in characters) of `offset`.
    pub fn line_col(&self, source: &str, offset: u32) -> (usize, usize) {
        let line = self.starts.partition_point(|start| *start <= offset) - 1;
        let start = self.starts[line] as usize;
        let column = source[start..offset as usize].chars().count();
        (line + 1, column + 1)
    }
}
