# probabilistic-machines

Sketches that answer questions about a stream in less memory than the stream.
`no_std`, zero dependencies.

Every structure is sized by const generics, so the state is one fixed array and
nothing allocates:

```rust
use probabilistic_machines::{CountMinSketch, HyperLogLog};

// 4096 registers: 4 KiB, 1.63% standard error, any cardinality.
let mut visitors = HyperLogLog::<4096>::new();
let mut requests = CountMinSketch::<2048, 5>::new();

for id in 0_u32..50_000 {
    let key = (id % 12_000).to_le_bytes();
    visitors.add(&key);
    requests.add(&key);
}

let error = (visitors.estimate() - 12_000.0).abs() / 12_000.0;
assert!(error < 0.05, "{error}");

// Count-min never undercounts.
assert!(requests.estimate(&0_u32.to_le_bytes()) >= 5);
assert_eq!(requests.total(), 50_000);
```

Sketches merge, which is the point: every worker keeps its own and you fold them
at the end. Merging HyperLogLog or count-min is exact, as if one sketch had seen
both streams.

```rust
use probabilistic_machines::HyperLogLog;

let mut monday = HyperLogLog::<1024>::new();
let mut tuesday = HyperLogLog::<1024>::new();
for id in 0_u32..8_000 {
    monday.add(&id.to_le_bytes());
}
for id in 4_000_u32..12_000 {
    tuesday.add(&id.to_le_bytes());
}

let mut week = HyperLogLog::<1024>::new();
for id in 0_u32..12_000 {
    week.add(&id.to_le_bytes());
}

monday.merge(&tuesday);
assert_eq!(monday, week);
```

Quantiles come from `TDigest`, which keeps clusters whose weight is capped by
how close they sit to the tails, so p99 is sharper than the median:

```rust
use probabilistic_machines::TDigest;

let mut latency = TDigest::new(100);
for ms in 1..=100_000 {
    latency.add(f64::from(ms));
}

assert!((latency.quantile(0.5) - 50_000.0).abs() < 1_000.0);
assert!((latency.quantile(0.999) - 99_900.0).abs() < 100.0);
assert_eq!(latency.quantile(1.0), 100_000.0);
assert!(latency.len() < 1_000);
```

`CuckooFilter` answers membership and, unlike a Bloom filter, forgets; it never
denies an item it holds and admits an item it does not with probability near 3%:

```rust
use probabilistic_machines::CuckooFilter;

let mut revoked = CuckooFilter::<1024>::new();
revoked.insert(b"token-aa31")?;
assert!(revoked.contains(b"token-aa31"));

revoked.remove(b"token-aa31");
assert!(!revoked.contains(b"token-aa31"));
# Ok::<(), probabilistic_machines::cuckoo::Full>(())
```

`HeavyHitters` tracks what dominates a stream in `K` counters. After `n`
observations, every key seen more than `n / K` times is still in the table:

```rust
use probabilistic_machines::HeavyHitters;

let mut paths = HeavyHitters::<u32, 16>::new();
for request in 0..100_000_u32 {
    paths.add(request % 4_096);
    if request % 4 == 0 {
        paths.add(7);
    }
}

assert_eq!(paths.top().map(|hitter| hitter.key), Some(7));
assert!(paths.count(&7) >= 25_000);
```

Every sketch writes to a byte slice and reads back bit-exact, so one process can
build it and another can query it:

```rust
use probabilistic_machines::HyperLogLog;

let mut sketch = HyperLogLog::<64>::new();
sketch.add(b"one");
sketch.add(b"two");

let mut frame = [0_u8; HyperLogLog::<64>::ENCODED_LEN];
assert_eq!(sketch.encode(&mut frame), Ok(66));
assert_eq!(HyperLogLog::<64>::decode(&frame), Ok(sketch));
```

`HyperLogLog`, `CountMinSketch`, `CuckooFilter` and `HeavyHitters` need no
features at all and run on bare metal. `TDigest` needs `alloc`, since its
cluster list grows and shrinks. `std` adds `HyperLogLog::standard_error` and
`HeavyHitters::ranked`.

Accuracy is a function of the size you pick, and nothing here hides that:
HyperLogLog's standard error is `1.04 / sqrt(M)`; count-min overshoots by more
than `e * total / W` with probability at most `e^-D`; a t-digest cluster near
quantile `q` holds at most `4 * n * q * (1 - q) / compression` of the stream.
