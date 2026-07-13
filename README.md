<div align="center">

<pre>
███╗   ███╗██╗   ██╗███╗   ██╗██╗███╗   ██╗███╗   ██╗
████╗ ████║██║   ██║████╗  ██║██║████╗  ██║████╗  ██║
██╔████╔██║██║   ██║██╔██╗ ██║██║██╔██╗ ██║██╔██╗ ██║
██║╚██╔╝██║██║   ██║██║╚██╗██║██║██║╚██╗██║██║╚██╗██║
██║ ╚═╝ ██║╚██████╔╝██║ ╚████║██║██║ ╚████║██║ ╚████║
╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝╚═╝╚═╝  ╚═══╝╚═╝  ╚═══╝
</pre>

**A distributed in-memory cache, written in Rust.**

*Muninn — Odin's raven, whose name means "memory".*

</div>

---

Muninn is a Redis-compatible cache server built from scratch on Tokio. It speaks
[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/) over TCP, so
`redis-cli` and standard Redis tooling work against it out of the box.

The end goal is a sharded, multi-node cache with consistent hashing — built
incrementally, benchmarked honestly, and failure-tested along the way.

## Architecture

```
                      ┌──────────────────────────────────────────────────┐
                      │                   muninn node                    │
                      │                                                  │
  redis-cli ─────┐    │  ┌──────────┐   ┌────────────┐   ┌────────────┐  │
  client app ────┼──▶ │  │   tcp    │   │    resp    │   │   store    │  │
  client app ────┘    │  │ listener │──▶│   parser   │──▶│  ttl · lru │  │
                      │  └──────────┘   └────────────┘   └────────────┘  │
                      │       │                                          │
                      │       └── one async task per connection          │
                      └──────────────────────────────────────────────────┘

     planned: a client-side router mapping keys onto a ring of muninn
              nodes — consistent hashing with virtual nodes
```

Every connection gets its own async task and its own read buffer. The parser turns the
raw TCP byte stream into complete commands; everything past that point never touches a
socket.

## Status

Working now:

- Async TCP server — one Tokio task per connection
- RESP wire protocol — incremental framing and parsing over an accumulating buffer

Upcoming:

- `GET` / `SET` / `DELETE` against an in-memory store
- TTL with expiry
- Configurable memory limit with LRU eviction
- Consistent hashing across multiple nodes, with key routing
- Benchmarks — throughput and p50/p99 latency under load, kill-a-node test

## Quick start

```sh
cargo run
```

The server listens on `127.0.0.1:6379`:

```sh
$ redis-cli -p 6379 SET name muninn
got command: ["SET", "name", "muninn"]     # command execution lands next
```

Or raw, over `nc`:

```sh
printf '*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$6\r\nmuninn\r\n' | nc 127.0.0.1 6379
```

## Protocol

Commands are RESP arrays of bulk strings — length-prefixed, so values can hold any
bytes without escaping. The parser deals with the realities of TCP as a byte stream:

- a command split across packets is buffered until complete
- multiple commands arriving in one packet are handled in order
- malformed or oversized frames close the connection with an error

The parser is pure (`&[u8]` in, verdict out), which keeps it unit-testable without a
socket in sight:

```sh
cargo test
```

## Design notes

Decisions so far, and the reasoning:

- **Task per connection over an event loop.** Redis multiplexes everything on one
  thread; muninn leans on Tokio's scheduler instead. Simpler code, and it makes the
  shared-state problem explicit — which is half the point of building this.
- **RESP over a custom protocol.** Compatibility with existing tooling is worth more
  than protocol novelty. Debugging with `redis-cli` beats writing a custom client.
- **Frame size limits at the parser.** Length headers come from the network and are
  treated as hostile: argument counts and bulk lengths are capped before any allocation
  happens.

Benchmark numbers and trade-off discussion will land here as the project progresses.
