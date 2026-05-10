use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

const MAX_UI_INSTANCE_FRAME_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum UiInstanceRequest {
    Activate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum UiInstanceReply {
    Ok,
    Error { message: String },
}

pub(super) fn write_json_line<T: Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let payload = serde_json::to_string(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    writer.write_all(payload.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

pub(super) fn read_json_line<T: for<'de> Deserialize<'de>>(
    reader: &mut impl BufRead,
) -> io::Result<T> {
    read_json_line_with_limit(reader, MAX_UI_INSTANCE_FRAME_BYTES)
}

fn read_json_line_with_limit<T: for<'de> Deserialize<'de>>(
    reader: &mut impl BufRead,
    max_frame_bytes: usize,
) -> io::Result<T> {
    let mut line = Vec::new();
    read_bounded_line(reader, &mut line, max_frame_bytes)?;
    serde_json::from_slice(&line).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    max_frame_bytes: usize,
) -> io::Result<()> {
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            if line.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "ui single-instance peer closed the connection",
                ));
            }
            return Ok(());
        }

        let consumed = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => position + 1,
            None => buffer.len(),
        };
        if line.len().saturating_add(consumed) > max_frame_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("UI single-instance frame exceeds {max_frame_bytes} bytes"),
            ));
        }

        line.extend_from_slice(&buffer[..consumed]);
        reader.consume(consumed);

        if line.last() == Some(&b'\n') {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn read_json_line_accepts_valid_request() {
        let mut reader = Cursor::new(
            br#"{"type":"activate"}
"#,
        );

        let request: UiInstanceRequest = read_json_line(&mut reader).unwrap();

        assert!(matches!(request, UiInstanceRequest::Activate));
    }

    #[test]
    fn read_json_line_accepts_eof_after_complete_json() {
        let mut reader = Cursor::new(br#"{"type":"ok"}"#);

        let reply: UiInstanceReply = read_json_line(&mut reader).unwrap();

        assert!(matches!(reply, UiInstanceReply::Ok));
    }

    #[test]
    fn read_json_line_rejects_empty_connection() {
        let mut reader = Cursor::new(Vec::<u8>::new());

        let err = read_json_line::<UiInstanceRequest>(&mut reader).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_json_line_rejects_oversized_frame() {
        let mut reader = Cursor::new(
            br#"{"type":"activate"}
"#,
        );

        let err = read_json_line_with_limit::<UiInstanceRequest>(&mut reader, 4).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err
            .to_string()
            .contains("UI single-instance frame exceeds 4 bytes"));
    }
}
