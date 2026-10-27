use super::*;

fn frames(text: &[u8], width: usize, limit: usize) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut parser = StreamParser::new(limit);
    let mut result = Vec::new();
    for bytes in text.chunks(width) {
        let mut chunk = Bytes::copy_from_slice(bytes);
        while let Some(event) = parser.next(&mut chunk)? {
            result.push((event.kind, event.data));
        }
    }
    if let Some(event) = parser.finish()? {
        result.push((event.kind, event.data));
    }
    Ok(result)
}

#[test]
fn utf8_bom_multiline_and_every_line_ending_survive_arbitrary_chunks() {
    for ending in ["\n", "\r\n", "\r"] {
        let text="\u{feff}: heartbeat\nid: cursor\nretry: 1\ndata: {\"ready\":\ndata: \"caf\u{e9}\u{1f30a}\"}\n\nevent: crucible.complete\ndata: {}\n\n".replace('\n',ending);
        for width in 1..=text.len() {
            let actual = frames(text.as_bytes(), width, 1024).unwrap();
            assert_eq!(
                actual,
                vec![
                    (
                        String::new(),
                        "{\"ready\":\n\"caf\u{e9}\u{1f30a}\"}".as_bytes().to_vec()
                    ),
                    ("crucible.complete".into(), b"{}".to_vec())
                ],
                "{ending:?} width{width}"
            );
        }
    }
}
#[test]
fn limits_include_framing_utf8_and_crlf_without_collecting_long_lines() {
    let frame = b"data: {}\r\n\r\n";
    assert!(frames(frame, 1, frame.len() - 1).is_err());
    assert!(frames(frame, 1, frame.len()).is_ok());
    let mut parser = StreamParser::new(256);
    let mut chunk = Bytes::from(vec![b'x'; 100_000]);
    assert!(parser.next(&mut chunk).err().unwrap().contains("limit"));
    assert_eq!(parser.line.len(), 256);
    assert_eq!(chunk.len(), 100_000 - 257);
}
#[test]
fn invalid_and_incomplete_utf8_are_rejected() {
    for bytes in [b"data: \xff\n\n".as_slice(), b"data: \xe2".as_slice()] {
        for width in [1, 2, 4096] {
            assert!(frames(bytes, width, 256).unwrap_err().contains("UTF-8"));
        }
    }
}

#[test]
fn bare_cr_dispatches_before_any_following_byte_and_split_lf_keeps_frame_bounds() {
    let mut parser = StreamParser::new(256);
    let mut chunk = Bytes::from_static(b"data: {}\r\r");
    assert_eq!(parser.next(&mut chunk).unwrap().unwrap().data, b"{}");
    assert!(chunk.is_empty());
    assert!(parser.next(&mut chunk).unwrap().is_none());
    let mut chunk = Bytes::from_static(b"\ndata: {}\r\r");
    assert_eq!(parser.next(&mut chunk).unwrap().unwrap().data, b"{}");
    assert!(chunk.is_empty());

    let mut parser = StreamParser::new(10);
    let mut chunk = Bytes::from_static(b"data: {}\r\r");
    assert!(parser.next(&mut chunk).unwrap().is_some());
    let mut lf = Bytes::from_static(b"\n");
    assert!(parser.next(&mut lf).err().unwrap().contains("limit"));
}
