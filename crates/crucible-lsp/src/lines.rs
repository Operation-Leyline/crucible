//! Byte offsets to LSP positions. LSP counts columns in UTF-16 code units by
//! default; `crucible-lang` spans are byte offsets.

use lsp_types::{Position, Range};

pub struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        Self { text, starts }
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self.starts.partition_point(|start| *start <= offset) - 1;
        let start = self.starts[line];
        let column: usize = self.text[start..offset].chars().map(char::len_utf16).sum();
        Position::new(line as u32, column as u32)
    }

    pub fn range(&self, start: u32, end: u32) -> Range {
        Range::new(self.position(start as usize), self.position(end as usize))
    }

    /// The byte offset of `position`, clamped to its line.
    pub fn offset(&self, position: Position) -> usize {
        let Some(&start) = self.starts.get(position.line as usize) else {
            return self.text.len();
        };
        let end = self
            .starts
            .get(position.line as usize + 1)
            .map_or(self.text.len(), |next| next - 1);
        let mut units = 0;
        for (index, ch) in self.text[start..end].char_indices() {
            if units >= position.character as usize {
                return start + index;
            }
            units += ch.len_utf16();
        }
        end
    }

    /// The whole first line: where problems with no better place are shown.
    pub fn first_line(&self) -> Range {
        let end = self.starts.get(1).map_or(self.text.len(), |next| next - 1);
        let end = self.text[..end].trim_end_matches('\r').len();
        self.range(0, end as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_count_utf16_units() {
        let lines = Lines::new("a\n// é😀x\n");
        let x = "a\n// é😀x".len() - 1;
        assert_eq!(lines.position(x), Position::new(1, 6));
        assert_eq!(lines.offset(Position::new(1, 6)), x);
        assert_eq!(lines.offset(Position::new(1, 99)), "a\n// é😀x".len());
        assert_eq!(lines.offset(Position::new(9, 0)), "a\n// é😀x\n".len());
    }
}
