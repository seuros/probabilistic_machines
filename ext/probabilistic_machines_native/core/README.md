# probabilistic-machines

Sketches that answer questions about a stream in less memory than the stream.
`no_std`, `unsafe` forbidden, allocation-free except `TDigest` (behind the
`alloc` feature). One dependency: `sliderule-machines`, for the integer
logarithm linear counting needs.

Every other structure is sized by const generics, so the state is one fixed
array and nothing allocates:

```rust
use probabilistic_machines::{CountMinSketch, HyperLogLog};

// 4096 registers: 4 KiB, 1.63% standard error, any cardinality.
let mut visitors = HyperLogLog::<4096>::new();
let mut requests = CountMinSketch::<2048, 5>::new();

for id in 0_u32..50_000 {
    let key = (id % 12_000).to_le_bytes();
    visitors.add(&key);
    requests.add(&key)?;
}

let error = (visitors.estimate() - 12_000.0).abs() / 12_000.0;
assert!(error < 0.05, "{error}");

// Count-min never undercounts.
assert!(requests.estimate(&0_u32.to_le_bytes()) >= 5);
assert_eq!(requests.total(), 50_000);
# Ok::<(), probabilistic_machines::Refusal>(())
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

let mut latency = TDigest::new(100).ok_or("compression below 10")?;
for ms in 1..=100_000 {
    latency.add(f64::from(ms))?;
}

assert!((latency.quantile(0.5) - 50_000.0).abs() < 1_000.0);
assert!((latency.quantile(0.999) - 99_900.0).abs() < 100.0);
assert_eq!(latency.quantile(1.0), 100_000.0);
assert!(latency.len() < 1_000);
# Ok::<(), Box<dyn core::error::Error>>(())
```

`CuckooFilter` answers membership and, unlike a Bloom filter, forgets; it never
denies an item it holds and admits an item it does not with probability near 3%:

```rust
use probabilistic_machines::{CuckooFilter, Removing};

let mut revoked = CuckooFilter::<1024>::new();
revoked.insert(b"token-aa31")?;
assert!(revoked.contains(b"token-aa31"));

assert_eq!(revoked.remove(b"token-aa31"), Removing::Cleared);
assert!(!revoked.contains(b"token-aa31"));
assert_eq!(revoked.remove(b"token-aa31"), Removing::NotHeld);
# Ok::<(), probabilistic_machines::Refusal>(())
```

The filter stores eight-bit fingerprints, nothing else, so both answers of
`remove` are about the fingerprint. `NotHeld` is a proof: there are no false
negatives, so an item whose fingerprint is in neither of its buckets is not
there. `Cleared` is not proof the item was inserted: about 3% of items never
inserted share a fingerprint and a bucket with a stored one (the same false
positive `contains` has), and removing one clears the stored item's
fingerprint, which the filter then denies. Remove only what you inserted.

`HeavyHitters` tracks what dominates a stream in `K` counters. After `n`
observations, every key seen more than `n / K` times is still in the table:

```rust
use probabilistic_machines::HeavyHitters;

let mut paths = HeavyHitters::<u32, 16>::new();
for request in 0..100_000_u32 {
    paths.add(request % 4_096)?;
    if request % 4 == 0 {
        paths.add(7)?;
    }
}

assert_eq!(paths.top().map(|hitter| *hitter.key()), Some(7));
assert!(paths.count(&7) >= 25_000);
# Ok::<(), probabilistic_machines::Refusal>(())
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

A key need only be `Clone` and `Eq`, so owned keys work without interning:
`HeavyHitters<String, 64>` tracks layer names as they are. Cloning happens on
insert and on `merge`, never on a lookup, so a cheap key is worth having but a
`String` is not a mistake. Interning to a `u32`, or hashing with `hash64` the
way the Ruby binding does, keeps the table `Copy` and the scan cache-friendly.

`HeavyHitters` codes only with `u64` keys, which is what a hash-keyed heavy
hitter is; a general `T` has no byte form.

`HyperLogLog`, `CountMinSketch`, `CuckooFilter` and `HeavyHitters` need no
features at all and run on bare metal. `TDigest` needs `alloc`, since its
cluster list grows and shrinks. `alloc` also adds `encoded`, which hands back
an exactly-sized frame instead of filling a caller's buffer, so nothing has to
thread an error that cannot happen:

```rust
use probabilistic_machines::CuckooFilter;

let mut revoked = CuckooFilter::<64>::new();
revoked.insert(b"token-aa31")?;

let frame = revoked.encoded();
assert_eq!(frame.len(), CuckooFilter::<64>::ENCODED_LEN);
assert_eq!(CuckooFilter::<64>::decode(&frame), Ok(revoked));
# Ok::<(), probabilistic_machines::Refusal>(())
```

`decode_into` is the reading half of the same idea: it reads a frame over a
structure the caller already owns, so a 128 KiB sketch behind a `Box` is never
copied through the stack.

`alloc` also adds `HeavyHitters::ranked`. There is no `std` feature:
`HyperLogLog::standard_error` needs no square root, since `M` is a power of two.

Accuracy is a function of the size you pick, and nothing here hides that:
HyperLogLog's standard error is `1.04 / sqrt(M)`; count-min overshoots by more
than `e * total / W` with probability at most `e^-D`; a t-digest cluster near
quantile `q` holds at most `4 * n * q * (1 - q) / compression` of the stream.
`TDigest::quantile` answers `f64::NAN` for an empty digest and for a `q`
outside `0.0..=1.0`, NaN included.

## Contract

### Refusals

Every operation that can refuse returns `Result<_, Refusal>`, and `Refusal` is
the crate's only refusal type. A refused call changes nothing: a test for each
one compares the whole structure against a clone taken before the call.

| `Refusal` | Meaning |
| --- | --- |
| `Truncated` | The buffer is shorter than the frame: `encode`'s output or `decode`'s input. |
| `WrongStructure` | The frame's tag names another structure. |
| `WrongShape` | The frame, or the sketch given to `TDigest::merge`, was built at another shape. |
| `Malformed` | The frame has the right shape but contents no sequence of calls produces. |
| `Full` | A cuckoo filter found no slot, in two buckets or after 64 evictions. |
| `Overflow` | A counter or a total would pass its type: a `u32` count-min counter, a `u64` heavy-hitters counter, or a digest's total weight becoming infinite. |
| `ZeroCount` | A count of zero, which observes nothing (and would evict a heavy hitter). |
| `NotFinite` | A digest value or weight is NaN or infinite. |
| `InvalidWeight` | A digest weight is zero or negative. |

### Every answer

| Call | Honoured | Refusals |
| --- | --- | --- |
| `HyperLogLog::add`, `add_hash`, `merge` | `()` | none: registers only rise, so both are idempotent |
| `CountMinSketch::add`, `add_count`, `add_hash_count` | `()` | `ZeroCount` (not `add`), `Overflow` |
| `CountMinSketch::merge` | `()` | `Overflow` |
| `CuckooFilter::insert`, `insert_hash` | `()` | `Full` |
| `CuckooFilter::remove`, `remove_hash` | `Removing::Cleared`, `Removing::NotHeld` (*repeat*) | none |
| `HeavyHitters::add`, `add_count` | `()` | `ZeroCount` (not `add`), `Overflow` |
| `HeavyHitters::merge` | `()` | `Overflow` |
| `TDigest::new` | `Some(digest)` | `None` below `MIN_COMPRESSION` (10) |
| `TDigest::add`, `add_weighted` | `()` | `NotFinite`, `InvalidWeight` (not `add`), `Overflow` |
| `TDigest::merge` | `()` | `WrongShape` (another compression), `Overflow` |
| `encode` (every sketch) | bytes written | `Truncated` |
| `decode`, `decode_into` (every sketch) | the sketch / `()` | `Truncated`, `WrongStructure`, `WrongShape` (not the digest), `Malformed` (not the cuckoo filter) |
| `Hitter::new`, `Centroid::new` | `Some(value)` | `None` for a value no table or digest holds |

`Hitter` and `Centroid` keep their fields private behind those constructors
and are read through `key()`, `count()`, `overestimate()`, `mean()` and
`weight()`, so no value a caller holds escaped the check.

The one honoured repeat is `Removing::NotHeld`: removing an item whose
fingerprint is in neither of its buckets. The filter proves it from its own
table (no false negatives) and changes nothing. A repeated `insert` is not a
repeat: the filter is a multiset of fingerprints and cannot tell a second copy
of an item from a colliding one, so it stores another copy.

No sketch reads or records a clock, and none issues ids: there is no time unit,
no scope and nothing to resume after a restart beyond `decode`.

### Determinism

`HyperLogLog::estimate` gives the same bits for the same registers on every
target with IEEE 754 binary64 arithmetic. Linear counting, under `2.5 * M`,
takes `ln(M / untouched)` from `sliderule_machines::ln_ratio` in 58-bit fixed
point (within `3 * 2^-58` of the exact logarithm) and converts it to `f64` with
one rounding, so the estimate is within `M * (ln(M / untouched) * 2^-53 +
3 * 2^-58)` of the exact `M * ln(M / untouched)`: under `10^-10` items at 65536
registers, against a documented error of `1.04 / sqrt(M)` of the estimate. The
earlier private `f64` series was within a few parts in `10^10` and depended on
the target's floating-point evaluation; this one does not. The harmonic-mean
branch is sums, products and one division of `f64`s, each correctly rounded.
Every `f64` answer here (estimates, quantiles, cdf) assumes binary64 evaluated
one rounding per operation; x87 extended-precision evaluation (32-bit x86
without SSE2) may differ in the last bit. Frames store `f64`s as their
little-endian bits, so a frame decodes to the same state everywhere.
