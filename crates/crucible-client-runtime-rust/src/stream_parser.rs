//! Incremental UTF-8 SSE framing, independent of response cardinality.
use crate::stream_event::StreamEvent;
use bytes::{Buf, Bytes};

pub(crate) struct StreamParser {
    line: Vec<u8>,
    data: Vec<u8>,
    kind: String,
    first: bool,
    cr: Option<usize>,
    bytes: usize,
    limit: usize,
}
impl StreamParser {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            line: Vec::new(),
            data: Vec::new(),
            kind: String::new(),
            first: true,
            cr: None,
            bytes: 0,
            limit,
        }
    }
    fn count(&mut self) -> Result<(), String> {
        if self.bytes == self.limit {
            return Err("stream event exceeds the byte limit".into());
        }
        self.bytes += 1;
        Ok(())
    }
    pub(crate) fn next(&mut self, chunk: &mut Bytes) -> Result<Option<StreamEvent>, String> {
        while !chunk.is_empty() {
            self.skip_lf(chunk)?;
            let Some(&byte) = chunk.first() else {
                break;
            };
            chunk.advance(1);
            self.count()?;
            match byte {
                b'\r' => {
                    // CR completes the line now; an idle producer need not send
                    // another byte. Only a possible LF remains to be skipped.
                    self.cr = Some(self.bytes);
                    if let Some(event) = self.line()? {
                        return Ok(Some(event));
                    }
                }
                b'\n' => {
                    if let Some(event) = self.line()? {
                        return Ok(Some(event));
                    }
                }
                _ => self.line.push(byte),
            }
        }
        Ok(None)
    }
    pub(crate) fn skip_lf(&mut self, chunk: &mut Bytes) -> Result<(), String> {
        if let Some(&byte) = chunk.first()
            && let Some(bytes) = self.cr.take()
            && byte == b'\n'
        {
            // A blank line reset the frame count before its optional LF
            // arrived. Charge that LF to the completed frame, not the next one.
            if bytes == self.limit {
                return Err("stream event exceeds the byte limit".into());
            }
            if self.bytes != 0 {
                self.count()?;
            }
            chunk.advance(1);
        }
        Ok(())
    }
    pub(crate) fn finish(&mut self) -> Result<Option<StreamEvent>, String> {
        self.cr = None;
        std::str::from_utf8(&self.line).map_err(|_| "invalid UTF-8 in response stream")?;
        Ok(None)
    }
    fn line(&mut self) -> Result<Option<StreamEvent>, String> {
        let mut line =
            std::str::from_utf8(&self.line).map_err(|_| "invalid UTF-8 in response stream")?;
        if self.first {
            self.first = false;
            line = line.strip_prefix('\u{feff}').unwrap_or(line);
        }
        let result = if line.is_empty() {
            self.bytes = 0;
            let kind = std::mem::take(&mut self.kind);
            if self.data.is_empty() {
                None
            } else {
                self.data.pop();
                Some(StreamEvent {
                    kind,
                    data: std::mem::take(&mut self.data),
                })
            }
        } else {
            if !line.starts_with(':') {
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => self.kind = value.to_owned(),
                    "data" => {
                        self.data.extend_from_slice(value.as_bytes());
                        self.data.push(b'\n');
                    }
                    _ => (),
                }
            }
            None
        };
        self.line.clear();
        Ok(result)
    }
}

#[cfg(test)]
#[path = "stream_parser_tests.rs"]
mod tests;
