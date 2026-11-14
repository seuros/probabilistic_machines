# ProbabilisticMachines 🔭

> **Survey the whole stream on a fixed fuel budget.** The tender that answers how
> many, how often and how extreme without carrying the cargo it measured.

Distinct counts, quantiles and frequencies in memory you pick up front.

Counting exactly means keeping every key: a `Set` of user ids, a `Hash` of path
counts, an array of latencies to sort. These five structures answer the same
questions from state that was sized at construction time and never grows.
**AMS Flajolet** takes a reading and discards the sample, which is why its hold
is the same size on the way home as it was at launch.

---

## 🚀 Launch Sequence

```ruby
require 'probabilistic_machines'

visitors = ProbabilisticMachines::HyperLogLog.new(4096)   # 4 KiB
50_000.times { |id| visitors.add("user-#{id % 12_000}") }

visitors.estimate       # => 12_361.2
visitors.standard_error # => 0.01625
```

---

## 🔩 The Native Extension Is Required

There is no pure-Ruby fallback and no `DISABLE_*` switch. HyperLogLog, Count-Min
Sketch, t-digest, cuckoo hashing and Space-Saving are five algorithms, and a
second implementation would mean maintaining each of them twice, with two sets
of accuracy tests and two byte formats to keep in step. The gem is a binding to
the `probabilistic-machines` crate in `ext/probabilistic_machines_native/core`,
and it needs Ruby 4.0 and a Rust toolchain (or a platform gem).

---

## 📐 Sizes Are a Fixed Menu

The crate sizes every structure by a const generic, so each size is a separately
compiled type. The constructor takes the size positionally, as the Rust type
takes its parameters; the extension carries the sizes in each class's constant
and raises `RangeError` for anything else (`TypeError` for a non-Integer):

| Class            | Constructor                      | Supported (constant)                                 |
| ---------------- | -------------------------------- | ---------------------------------------------------- |
| `HyperLogLog`    | `new(registers)`                 | `REGISTERS`: 1024, 4096, 16384, 65536                |
| `CountMinSketch` | `new(width, depth)`              | `WIDTHS`: 1024, 4096 by `DEPTHS`: 4, 8 (every pair)  |
| `CuckooFilter`   | `new(buckets)`                   | `BUCKETS`: 1024, 4096, 16384, 65536                  |
| `HeavyHitters`   | `new(counters)`                  | `COUNTERS`: 16, 64, 256                              |
| `TDigest`        | `new(compression)`               | `MIN_COMPRESSION` (10) through 65535                 |

```ruby
ProbabilisticMachines::HyperLogLog.new(3000)
# RangeError: registers 3000 is not supported, expected one of [1024, 4096, 16384, 65536]
```

---

## 🌌 How Many Distinct

```ruby
hll = ProbabilisticMachines::HyperLogLog.new(4096)
hll.add('user-1')
hll.add_hash(0x9e37_79b9_7f4a_7c15) # for streams that ship hashes, not keys
hll.estimate                        # Float
hll.standard_error                  # 1.04 / sqrt(registers)
hll.merge(other)                    # :merged, exact union; :wrong_shape for another size
```

Nothing here refuses but a merge across sizes: registers only rise, so `add`
and `merge` are idempotent and have no limit to hit. The estimate is the same
Float on every machine: linear counting takes its logarithm in integer fixed
point from `sliderule-machines`, within `10^-10` items of the exact value.

Memory is one byte per register, so 4096 registers cost 4 KiB whether the stream
held ten ids or ten billion.

---

## 📊 How Often Each

```ruby
cms = ProbabilisticMachines::CountMinSketch.new(4096, 4)
cms.add('/login')             # :added
cms.add_count('/login', 3)    # :added, or :zero_count / :overflow
cms.add_hash_count(hash, 3)   # for streams that ship hashes
cms.estimate('/login')        # never below the truth
cms.estimate_hash(hash)
cms.total                     # every count added
cms.merge(other)              # :merged, exact sum; :overflow, :wrong_shape
```

A counter is 32 bits. An addition or merge that would carry one past
`2**32 - 1` answers `:overflow` and changes nothing, so a counter never sticks
below the truth and the lower bound always holds.

A count is over the truth by at most `e * total / width`, with probability
`1 - e ** -depth` of staying inside it. Width buys accuracy, depth buys
confidence.

---

## 📏 Quantiles

```ruby
td = ProbabilisticMachines::TDigest.new(100)
td.add(12.5)                      # :added, or :not_finite / :overflow
td.add_weighted(12.5, 1_000.0)    # one point standing for a thousand observations
td.quantile(0.99)
td.cdf(12.5)
td.centroids                      # Array of frozen Centroid(mean, weight)
td.minimum                        # exact, as is maximum
td.compress
td.merge(other)                   # :merged; :wrong_shape for another compression
```

A NaN or infinite value or weight is `:not_finite`, a zero or negative weight
`:invalid_weight`, a total weight that would become infinite `:overflow`; each
changes nothing. Digests of different compressions do not merge: compression
only joins clusters, so the coarser one's clusters would break the finer one's
weight bound.

A quantile is off by about `1 / compression` of the distribution in the middle
and is exact at the extremes, in `compression * ln(n)` clusters rather than `n`
samples. An empty digest answers `Float::NAN`.

---

## 🛰️ Have I Seen This Contact Before

```ruby
cf = ProbabilisticMachines::CuckooFilter.new(4096)
cf.insert('device-9')      # :inserted, or :full when wedged
cf.contains?('device-9')
cf.remove('device-9')      # :cleared, or :not_held; unlike a Bloom filter, it can forget
cf.insert_hash(hash)       # and contains_hash?, remove_hash
```

`buckets * 4` items in `buckets * 4` bytes. Never a false negative; about 3%
false positives. The filter stores fingerprints and nothing else, so `remove`
answers about the fingerprint: `:not_held` proves it is in neither of the
item's buckets (nothing changed), while `:cleared` is no proof the item was
inserted, because a colliding item's fingerprint looks the same. Only remove
items you inserted, or you take the colliding item's fingerprint with it.
Inserting an item twice stores two copies. A cuckoo filter cannot merge, so
there is no `merge` here.

---

## 👑 Who Dominates the Stream

```ruby
hh = ProbabilisticMachines::HeavyHitters.new(64)
hh.add('10.0.0.1')              # :added
hh.add_count('10.0.0.1', 5)     # :added, or :zero_count / :overflow
hh.top                          # Hitter(key, count, overestimate), nil when empty
hh.ranked                       # every tracked key, most frequent first
hh.hitters                      # every tracked key, in table order
hh.observed                     # observations fed in
hh.merge(other)                 # :merged; :overflow, :wrong_shape
```

After `n` observations every key seen more than `n / k` times is tracked, in `k`
counters and no heap per key. A counter can inherit the count of the key it
evicted, or what a merged-in table's own evictions may have hidden, so the true
count lies between `count - overestimate` and `count`.

Keys are UTF-8 strings here. Space-Saving takes any `Clone + Eq` key in Rust;
the extension counts 64-bit key hashes, so the table stays one fixed array, and
keeps the string each tracked hash came from, to hand back in each `Hitter`. A
key that is not exactly a String (a subclass instance or a `to_str` object
included) is a `TypeError`, one that is not UTF-8 text a `RangeError` (a valid
UTF-8 String, or ASCII bytes in any ASCII-compatible encoding; a binary String
holding UTF-8 bytes is not transcoded), and the key a `Hitter` holds is a
frozen copy. The other four classes take exactly a String of any bytes in any
encoding, read natively (no `#bytes` or `#to_str` runs). A zero count is `:zero_count`: it observes nothing, yet on a full
table it would evict the weakest key.

---

## 📦 Transmitting a Reading

`encoded` returns the crate's own byte frame, `decode` reads one back, and
`decode_into` reads one over a sketch you already hold. The frame names its own
shape, so `decode` needs no size. It reads the frame's fields against each
other and refuses a state no stream could have left behind, so an edited blob
answers `:malformed` instead of handing back a sketch that counts more than it
ever saw. Like every refusal, these are symbols, never exceptions.

```ruby
blob = hll.encoded                                  # binary String
ProbabilisticMachines::HyperLogLog.decode(blob)     # a HyperLogLog
ProbabilisticMachines::TDigest.decode(blob)         # => :wrong_structure
hll.decode_into(other_size.encoded)                 # => :wrong_shape, hll untouched
```

Anything that reads the crate reads these frames, so an embedded device can ship
a sketch a Rails app folds in. `HeavyHitters` is the one blob that is not purely
the crate's: the crate's frame counts 64-bit key hashes, so the extension appends
its key table after it, one record of a 64-bit byte length and that many UTF-8
bytes per tracked key. `decode` rehashes each name, and answers `:malformed` for
a key table that is cut short or does not name every key the frame tracks.

```ruby
restored = ProbabilisticMachines::HeavyHitters.decode(hh.encoded)
restored.top.key # => '10.0.0.1', the string, not its hash
```

---

## 📜 The Contract

- **A refusal is a symbol and changes nothing.** It is the snake_case of the
  core's `Refusal` variant; there is no catch-all symbol and no refusal raises.
- **Exceptions are for malformed calls.** Wrong type: `TypeError`. An Integer
  outside its range, a size off the menu, a compression below 10, a `Hitter`
  or `Centroid` the core would not build: `RangeError`. Mutating a frozen
  sketch: `FrozenError`. `ArgumentError` only for a wrong number of arguments.
- **Values are valid or do not exist.** `Hitter.new(key, count, overestimate)`
  needs `overestimate < count`; `Centroid.new(mean, weight)` a finite mean and
  a finite, positive weight. A value you build is held to the same rule, by the
  core, as one the gem returns. Both classes are sealed: subclassing one raises
  `TypeError` and `allocate` is undefined. Sealing stops accidents, not
  reflection (a named subclass's constant is bound before the raise,
  `Marshal.load` skips `initialize`); the sketches only build these values and
  take none back, so a forged one has no way into a sketch. The classes are
  resolved once, when the extension loads.
- **One honoured repeat.** `CuckooFilter#remove` of an item whose fingerprint
  is in neither bucket answers `:not_held` and changes nothing. Nothing else is
  a repeat: a second `insert` stores a second copy, a second `add` counts again.
- **No clock, no ids, no restarts.** No sketch reads time or issues a handle;
  what survives a restart is what you `encoded`.

### Every answer

| Call | Honoured | Refusals |
| --- | --- | --- |
| `HyperLogLog#add`, `#add_hash` | `nil` | none |
| `HyperLogLog#merge(other)` | `:merged` | `:wrong_shape` |
| `CountMinSketch#add(item)` | `:added` | `:overflow` |
| `CountMinSketch#add_count(item, n)`, `#add_hash_count(hash, n)` | `:added` | `:zero_count`, `:overflow` |
| `CountMinSketch#merge(other)` | `:merged` | `:overflow`, `:wrong_shape` |
| `CuckooFilter#insert(item)`, `#insert_hash(hash)` | `:inserted` | `:full` |
| `CuckooFilter#remove(item)`, `#remove_hash(hash)` | `:cleared`, `:not_held` (*repeat*) | none |
| `HeavyHitters#add(key)` | `:added` | `:overflow` |
| `HeavyHitters#add_count(key, n)` | `:added` | `:zero_count`, `:overflow` |
| `HeavyHitters#merge(other)` | `:merged` | `:overflow`, `:wrong_shape` |
| `TDigest#add(value)` | `:added` | `:not_finite`, `:overflow` |
| `TDigest#add_weighted(value, weight)` | `:added` | `:not_finite`, `:invalid_weight`, `:overflow` |
| `TDigest#merge(other)` | `:merged` | `:wrong_shape`, `:overflow` |
| `TDigest#compress` | `nil` | none |
| `.decode(bytes)` | the sketch | `:truncated`, `:wrong_structure`, `:wrong_shape` (not `TDigest`), `:malformed` (not `CuckooFilter`) |
| `#decode_into(bytes)` | `:decoded` | as `.decode` |

Queries (`estimate`, `contains?`, `count`, `top`, `quantile`, ...) answer values
and never refuse. No answer depends on or records a clock.

| Refusal | Meaning |
| --- | --- |
| `:truncated` | The blob is shorter than the frame it starts. |
| `:wrong_structure` | The blob is another sketch's frame. |
| `:wrong_shape` | The blob, or the sketch given to `merge`, has another size or compression. In Rust a merge across sizes does not compile. |
| `:malformed` | The blob holds contents no sequence of calls produces. |
| `:full` | The cuckoo filter found no slot after 64 evictions; expect it past about 95% of `capacity`. |
| `:overflow` | A counter or total would pass `2**32 - 1` (count-min), `2**64 - 1` (heavy hitters) or stay finite (t-digest weight). |
| `:zero_count` | A count of zero. |
| `:not_finite` | A t-digest value or weight is NaN or infinite. |
| `:invalid_weight` | A t-digest weight is zero or negative. |

---

## ⚙️ The Rust Core

The sketches themselves are the `probabilistic-machines` crate under
`ext/probabilistic_machines_native/core`: `no_std`, `unsafe` forbidden, one
dependency (`sliderule-machines`, for integer logarithms), allocating only for
`TDigest` and for the `encoded` frames this extension returns, and verified on
bare metal. The binding crate beside it under
`ffi/` is the Ruby surface and is not published.

---

## 📡 Credits

**AMS Flajolet**, NCC-0185, *Flajolet*-class tender.

Named for Philippe Flajolet and G. Nigel Martin's *Probabilistic Counting
Algorithms for Data Base Applications* (1985), and for Flajolet, Fusy, Gandouet
and Meunier's *HyperLogLog* (2007), which established that you can count what
you refuse to store.

---

## License

MIT.
