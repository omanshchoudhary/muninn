use crate::store::Store;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
enum ParseResult {
    Complete { args: Vec<String>, consumed: usize }, // Got the full command
    Incomplete,                                      // Incomplete Command but yeah not invalid
    Invalid(&'static str),
}

enum Command {
    Get(String),
    Set(String, String),
    Delete(String),
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

// Convert args to structured commands
fn parse_commands(args: Vec<String>) -> Result<Command, &'static str> {
    if args.is_empty() {
        return Err("empty command");
    }

    match args[0].to_uppercase().as_str() {
        "GET" => {
            if args.len() != 2 {
                return Err("wrong number of arguments for 'GET'");
            }
            return Ok(Command::Get(args[1].clone()));
        }
        "SET" => {
            if args.len() != 3 {
                return Err("wrong number of arguments for 'SET'");
            }
            return Ok(Command::Set(args[1].clone(), args[2].clone()));
        }
        "DELETE" => {
            if args.len() != 2 {
                return Err("wrong number of arguments for 'DELETE'");
            }
            return Ok(Command::Delete(args[1].clone()));
        }
        _ => {
            return Err("unknown command");
        }
    }
}

pub async fn handle(mut socket: TcpStream, store: Arc<Store>) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];

    loop {
        loop {
            match parse_frame(&buf) {
                ParseResult::Complete { args, consumed } => {
                    buf.drain(..consumed);
                    let reply = match parse_commands(args) {
                        Ok(Command::Get(key)) => match store.get(key) {
                            Some(value) => format!("${}\r\n{}\r\n", value.len(), value),
                            None => "$-1\r\n".to_string(),
                        },
                        Ok(Command::Set(key, value)) => {
                            store.set(key, value);
                            "+OK\r\n".to_string()
                        }
                        Ok(Command::Delete(key)) => {
                            if store.delete(key) {
                                ":1\r\n".to_string()
                            } else {
                                ":0\r\n".to_string()
                            }
                        }
                        Err(msg) => format!("-ERR {}\r\n", msg),
                    };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_complete(buf: &[u8]) -> (Vec<String>, usize) {
        match parse_frame(buf) {
            ParseResult::Complete { args, consumed } => (args, consumed),
            other => panic!("expected Complete, got {:?}", other),
        }
    }

    #[test]
    fn complete_command() {
        let buf = b"*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$6\r\nomansh\r\n";
        let (args, consumed) = expect_complete(buf);
        assert_eq!(args, ["SET", "name", "omansh"]);
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn incomplete_at_every_cut() {
        // chopping a valid command at *any* byte must give Incomplete, never Invalid
        let buf = b"*2\r\n$3\r\nGET\r\n$4\r\nname\r\n";
        for cut in 0..buf.len() {
            match parse_frame(&buf[..cut]) {
                ParseResult::Incomplete => {}
                other => panic!("cut at {} gave {:?}", cut, other),
            }
        }
    }

    #[test]
    fn glued_commands_consume_only_first() {
        let first = b"*2\r\n$3\r\nGET\r\n$4\r\nname\r\n";
        let mut buf = first.to_vec();
        buf.extend_from_slice(b"*1\r\n$4\r\nPING\r\n");

        let (args, consumed) = expect_complete(&buf);
        assert_eq!(args, ["GET", "name"]);
        assert_eq!(consumed, first.len()); // leftover bytes stay for the next parse

        let (args2, _) = expect_complete(&buf[consumed..]);
        assert_eq!(args2, ["PING"]);
    }

    #[test]
    fn empty_buffer_is_incomplete() {
        assert!(matches!(parse_frame(b""), ParseResult::Incomplete));
    }

    #[test]
    fn garbage_is_invalid() {
        assert!(matches!(
            parse_frame(b"hello there\r\n"),
            ParseResult::Invalid(_)
        ));
    }

    #[test]
    fn wrong_bulk_prefix_is_invalid() {
        assert!(matches!(
            parse_frame(b"*1\r\n#3\r\nGET\r\n"),
            ParseResult::Invalid(_)
        ));
    }

    #[test]
    fn missing_crlf_after_data_is_invalid() {
        assert!(matches!(
            parse_frame(b"*1\r\n$3\r\nGETXX"),
            ParseResult::Invalid(_)
        ));
    }

    #[test]
    fn huge_arg_count_is_rejected() {
        assert!(matches!(
            parse_frame(b"*99999999\r\n"),
            ParseResult::Invalid(_)
        ));
    }

    #[test]
    fn empty_value_is_ok() {
        let (args, _) = expect_complete(b"*2\r\n$3\r\nSET\r\n$0\r\n\r\n");
        assert_eq!(args, ["SET", ""]);
    }
}
