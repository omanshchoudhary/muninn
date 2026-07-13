use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
enum ParseResult {
    Complete { args: Vec<String>, consumed: usize }, // Got the full command
    Incomplete,                                      // Incomplete Command but yeah not invalid
    Invalid(&'static str),
}

fn read_line(buf: &[u8], pos: usize) -> Option<(&[u8], usize)> {
    let slice = &buf[pos..];
    let idx = slice.windows(2).position(|w| w == b"\r\n")?;
    Some((&slice[..idx], pos + idx + 2)) // returns the line content && start index for the next parsing part
}

fn parse_number(line: &[u8], prefix: u8) -> Option<usize> {
    if line.first() != Some(&prefix) {
        return None;
    }
    std::str::from_utf8(&line[1..]).ok()?.parse().ok()
}

fn parse_frame(buf: &[u8]) -> ParseResult {
    use ParseResult::*;
    let mut pos = 0;

    // Array Header: *<count>\r\n, count tells number of arguments
    let (line, next) = match read_line(buf, pos) {
        Some(v) => v,
        None => return Incomplete,
    };

    let count = match parse_number(line, b'*') {
        Some(n) => n,
        None => return Invalid("expected '*<count>'"),
    };

    if count > 1000 {
        return ParseResult::Invalid("Too many argument");
    }
    pos = next;

    // Arrays of count elements
    let mut args = Vec::with_capacity(count);

    // Each argument: $<len>\r\n<bytes>\r\n
    for _ in 0..count {
        let (line, next) = match read_line(buf, pos) {
            Some(v) => v,
            None => return Incomplete,
        };

        let len = match parse_number(line, b'$') {
            Some(n) => n,
            None => return Invalid("expected '$<len>'"),
        };
        pos = next;

        if buf.len() < pos + len + 2 {
            return Incomplete;
        }
        if &buf[pos + len..pos + len + 2] != b"\r\n" {
            return Invalid("missing CRLF after data");
        }
        match std::str::from_utf8(&buf[pos..pos + len]) {
            Ok(s) => args.push(s.to_string()),
            Err(_) => return Invalid("non-utf8 data"),
        }
        pos += len + 2;
    }
    Complete {
        args,
        consumed: pos,
    }
}

pub async fn handle(mut socket: TcpStream) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];

    loop {
        loop {
            match parse_frame(&buf) {
                ParseResult::Complete { args, consumed } => {
                    buf.drain(..consumed);
                    let reply = format!("+got command: {:?}\r\n", args);
                    if socket.write_all(reply.as_bytes()).await.is_err() {
                        return;
                    }
                }
                ParseResult::Incomplete => break, // need more bytes

                ParseResult::Invalid(msg) => {
                    let _ = socket
                        .write_all(format!("-ERR {}\r\n", msg).as_bytes())
                        .await;
                    return;
                }
            }
        }
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}
