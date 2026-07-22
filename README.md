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

*Muninn: Odin's raven, whose name means "memory".*

</div>

---

Muninn is a sharded in-memory cache built on Tokio. It speaks a
[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/) wire protocol over
TCP, stores keys with optional TTLs, evicts under memory pressure using LRU, and spreads
keys across multiple nodes using consistent hashing with virtual nodes.

Every number in this README comes from a benchmark in this repository and can be
reproduced with the commands at the bottom.

## Architecture

```
   ┌────────────────────────┐
   │      client app        │
   │  ┌──────────────────┐  │        ┌───────────────────────────────────┐
   │  │      Router      │  │        │            muninn node            │
   │  │                  │  │   ┌───▶│  tcp listener → resp → store      │
   │  │  ring: hash ring │──┼───┤    │                    ttl · lru      │
   │  │  conns: per node │  │   │    └───────────────────────────────────┘
   │  └──────────────────┘  │   │
   └────────────────────────┘   │    ┌───────────────────────────────────┐
                                ├───▶│            muninn node            │
   ┌────────────────────────┐   │    └───────────────────────────────────┘
   │   another client app   │   │
   │  ┌──────────────────┐  │   │    ┌───────────────────────────────────┐
   │  │  its own Router  │──┼───┘───▶│            muninn node            │
   │  └──────────────────┘  │        └───────────────────────────────────┘
   └────────────────────────┘
        routing is client-side       nodes are independent and know
        each client owns a ring      nothing about each other
```

A node is a whole process with its own `HashMap`, LRU, memory budget and socket. It has no
peer list, no gossip, and no knowledge that it belongs to a cluster. All cluster awareness
lives in the client's `Router`, which hashes the key, selects the node, and connects to it
directly.

Inside a node, every connection gets its own Tokio task. The parser converts a raw byte
stream into complete commands. Past that point nothing touches a socket.

## Quick start

```sh
cargo build --release
```

Run a single node:

```sh
./target/release/muninn --port 6379 --max-memory 104857600
```

`--max-memory` is in bytes. Omit it or pass `0` for unlimited, following the same
convention as Redis `maxmemory 0`. Talk to it over `nc`:

```sh
printf '*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$6\r\nmuninn\r\n' | nc 127.0.0.1 6379
```

Run a cluster by starting several nodes on different ports, then point a `Router` at all
of them:

```rust
let nodes = vec!["127.0.0.1:6379".into(), "127.0.0.1:6380".into()];
let mut r = Router::new(nodes, 150)?;   // 150 virtual nodes per physical node
r.set("user:42", "omansh")?;
r.get("user:42")?;
```

## Commands

| command | reply |
|---|---|
| `SET key value` | `+OK`, or `-ERR` if the value alone exceeds `--max-memory` |
| `SET key value EX secs` | `+OK`, expires after `secs` |
| `GET key` | bulk string, or `$-1` if missing or expired |
| `DELETE key` | `:1` if it existed, `:0` otherwise |

This is not a drop-in Redis replacement. It uses RESP framing but implements three
commands, and spells deletion `DELETE` rather than Redis `DEL`. `PING`, `COMMAND` and
everything else return `-ERR unknown command`, so `redis-cli` will not drive it cleanly.

## Benchmarks

All numbers from `src/bin/bench.rs` on an Intel i7-10610U (4 cores / 8 threads, 1.8GHz
base, 4.9GHz boost), 16 GB RAM, Ubuntu 24.04 on kernel 7.0, rustc 1.96, release build.
Clients and servers share one machine over loopback. This is a laptop CPU with aggressive
thermal and frequency scaling, so absolute throughput will vary between runs.

Workload: 10,000 keys, 64-byte values, 90% `GET` and 10% `SET`.

### Single node

```
160,000 ops in 1.282s  ->  124,767 ops/sec

latency (µs)      p50     p95     p99   p99.9
  GET              50      96     477     984
  SET              51      97     476     944
```

### Effect of sharding

With every process free to use all 8 cores, one node and three nodes perform the same. The
bottleneck is the client and the loopback syscalls, not the node's lock.

To measure what sharding provides, each node was pinned to a single core with `taskset`,
which models the case where a node itself is the constraint. Clients ran on the remaining
five cores.

| client threads | 1 node ops/s | 3 nodes ops/s | speedup | 1 node p50 | 3 nodes p50 |
|---:|---:|---:|---:|---:|---:|
| 8 | 114,759 | 132,583 | 1.16x | 71µs | 38µs |
| 16 | 129,951 | 138,811 | 1.07x | 125µs | 71µs |
| 32 | 115,205 | 149,119 | 1.29x | 263µs | 107µs |
| 64 | 110,597 | 152,112 | 1.38x | 557µs | 163µs |
| 128 | 99,995 | 178,380 | 1.78x | 1,201µs | 117µs |

The throughput column understates the effect. Past 32 threads a single node stops getting
faster and begins queueing: p50 climbs from 71µs to 1.2ms while throughput falls from 130k
to 100k ops/sec. The three-node cluster holds p50 near 100µs across the whole range.

Sharding does not raise peak throughput on this hardware. What it does is prevent queueing
once a node saturates, which is worth roughly 10x on p50 latency at 128 concurrent
clients.

Caveats: this is a closed-loop benchmark, where each thread waits for a reply before
sending again, so it undercounts tail latency compared to an open-loop load generator.
Everything runs over loopback, so there is no real network cost. Both make the absolute
numbers optimistic. The comparison between one and three nodes is the useful part.

## Rebalancing

`src/bin/remap.rs` writes 10,000 keys across three nodes, adds a fourth, and measures the
result:

```
keys that changed owner:                 24.3%
keys now on the new node:                24.3%
keys shuffled between existing nodes:        0
hit rate after adding a node:            75.7%
```

Modulo hashing (`hash(key) % n`) moves 75% of keys when going from three nodes to four,
which cold-starts almost the entire cache at once. Consistent hashing moves `1/n`, and
every key that moved went onto the new node. Nothing shuffled between existing nodes.

Muninn does not migrate data when the ring changes. It takes the miss. A database would
have to move those 24.3% of keys; a cache refetches them from the source, which makes
rebalancing free. Removing the node returns the hit rate to 100%, because the original
owners still hold their copies.

That property is also a hazard. Because the hash is deterministic, re-adding a node under
the same address restores its exact ring positions, and any writes made while it was away
become silently invisible. TTLs bound the damage. Nothing else does.

## Behaviour when a node dies

`src/bin/killnode.rs` spawns a three-node cluster, writes 3,000 keys, and sends `SIGKILL`
to one node:

```
all three nodes up
  hits  3000 (100.0%)   misses     0          errors     0

killing 127.0.0.1:7380 which owns 1016 keys

after SIGKILL, router unchanged
  hits  1984 ( 66.1%)   misses     0          errors  1016 (33.9%)
  first error: connection closed

after remove_node, ring reshaped
  hits  1984 ( 66.1%)   misses  1016 (33.9%)  errors     0

after rewriting the keyspace
  hits  3000 (100.0%)   misses     0          errors     0
```

The distinction that matters is between an error and a miss. A cache miss is routine: the
application falls back to its database. A connection error is an exception that propagates
to the caller.

Muninn has no failure detection, so a dead node converts a third of ordinary cache reads
into I/O errors. The client continues routing to an address that no longer answers,
indefinitely, because nothing informs it otherwise. Only an explicit `remove_node` call
downgrades those errors to ordinary misses, and that call has to come from outside the
system.

There is also no replication, so the 1,016 keys held by that node are lost.

## Design decisions

**Client-side routing.** The hash ring can live on the client, in a proxy, or on every
node (the Redis Cluster approach). Muninn puts it on the client, which keeps nodes
completely dumb and avoids an extra network hop. The cost is that every client must carry
the same node list, and nothing enforces this. A client with a stale list routes to the
wrong node and reads stale data with no error raised anywhere.

**Virtual nodes, 150 per physical node.** Placing each node on the ring once produces
uneven arcs and dumps a failed node's entire load onto one neighbour. 150 placements
brings the split within a few percent of even and spreads a failure across every survivor.
Expected imbalance scales as roughly `1/√vnodes`.

**FNV-1a with an avalanche finalizer.** `DefaultHasher` is randomly seeded per process, so
two clients would compute different rings for the same key. Plain FNV-1a is deterministic
but has weak avalanche: its high bits barely change when the last input byte changes, so
`node-a#0` through `node-a#149` all landed in one narrow slice of the ring and 450 virtual
nodes behaved like 3. A xor-shift and multiply finalizer corrects the distribution. Code
review did not catch this. The test that measured actual key spread did.

**Index-based LRU.** The textbook doubly linked list requires `Rc<RefCell<Node>>`, which is
not `Send` and therefore cannot cross a `tokio::spawn`. Nodes live in a `Vec` instead, with
`Option<usize>` indices as links and a free-list for slot reuse. Slots are never removed
from the `Vec`, since that would shift every index and invalidate every link.

**One `Mutex` covering map, byte counter and LRU together.** Only state inside the same
lock is guaranteed consistent together. Splitting them would allow a reader to observe a
key that the byte counter no longer accounts for.

**Node identity is its address.** A node is keyed on the ring by the same string used to
dial it. This is simple and correct as long as nodes do not move. A node that changes port
lands elsewhere on the ring and becomes a new node. Production systems separate node ID
from address for this reason.

**A synchronous client.** The server is async because it handles many connections
concurrently. The client issues one request and waits for one reply, so async provides no
benefit. Concurrency comes from one `Router` per thread, each with its own sockets.

## Limitations

- No failure detection. A dead node produces I/O errors until `remove_node` is called
  manually.
- No replication. A dead node's keys are lost.
- No node-list coordination. Clients with differing node lists disagree about ownership
  silently.
- `Router::new` fails if any node is down, so a client cannot start against a partially
  healthy cluster.
- Memory accounting counts key and value bytes only, excluding `HashMap` overhead and the
  LRU arena. Real usage exceeds `--max-memory`.
- Overwriting a key at the memory limit counts the old entry's bytes during the eviction
  loop, so it can evict more entries than necessary.

## Tests

```sh
cargo test                         # 25 unit tests
cargo run --release --bin killnode # failure test, spawns its own cluster

# these two need nodes already running
cargo run --release --bin bench -- --nodes 127.0.0.1:6379 --threads 32
cargo run --release --bin remap    # rebalancing, needs nodes on 6379-6382
```

The ring, LRU and protocol parser are pure functions of their inputs and are tested
without a socket. The distribution tests assert on measured key spread and remapping
percentages, which is how the hash bug described above was found.
