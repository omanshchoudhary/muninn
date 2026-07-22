use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;

pub enum Reply {
    Ok,            // +OK
    Value(String), // $5\r\nhello
    Nil,           // $-1
    Int(i64),      // :1
    Error(String), // -ERR ...
}

pub struct Connection {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Connection {
    pub fn connect(addr: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        // second handle to the same socket, one to write with one to read from
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { stream, reader })
    }

    pub fn request(&mut self, args: &[&str]) -> io::Result<Reply> {
        let encoded = encode(args);
        self.stream.write_all(&encoded)?;
        self.stream.flush()?;
        read_reply(&mut self.reader)
    }
}

// encode into bytes to send as client through tcp socket
fn encode(args: &[&str]) -> Vec<u8> {
    // Output buffer
    let mut out = Vec::new();

    let resp_header = format!("*{}\r\n", args.len());

    out.extend_from_slice(resp_header.as_bytes());

    for arg in args {
        // Bulk string header: $<len>\r\n
        out.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());

        // Argument: <arg>\r\n
        out.extend_from_slice(arg.as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    out
}

fn bad(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn read_reply(r: &mut impl BufRead) -> io::Result<Reply> {
    // every reply starts with one line: type byte + payload
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "connection closed",
        ));
    }
    let line = line.trim_end(); // read_line keeps the \r\n

    // peel off the type byte, rest is whatever follows
    let mut chars = line.chars();
    let kind = chars.next().ok_or_else(|| bad("empty reply line"))?;
    let rest = chars.as_str();

    match kind {
        '+' => Ok(Reply::Ok),
        '-' => Ok(Reply::Error(rest.to_string())),
        ':' => rest
            .parse::<i64>()
            .map(Reply::Int)
            .map_err(|_| bad("bad integer reply")),
        '$' => {
            let len: i64 = rest.parse().map_err(|_| bad("bad bulk length"))?;

            // nil has no payload line, reading one would eat the next reply
            if len < 0 {
                return Ok(Reply::Nil);
            }

            // header told us the length so read exactly that, +2 for the \r\n
            let mut buf = vec![0u8; len as usize + 2];
            r.read_exact(&mut buf)?;
            buf.truncate(len as usize);

            String::from_utf8(buf)
                .map(Reply::Value)
                .map_err(|_| bad("non-utf8 value"))
        }
        _ => Err(bad("unknown reply type")),
    }
}
