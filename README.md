# ProbabilisticMachines

Distinct counts, quantiles and frequencies in memory you pick up front.

Counting exactly means keeping every key: a `Set` of user ids, a `Hash` of path
counts, an array of latencies to sort. These five structures answer the same
questions from state that was sized at construction time and never grows.

```ruby
require 'probabilistic_machines'

visitors = ProbabilisticMachines::HyperLogLog.new(registers: 4096)   # 4 KiB
50_000.times { |id| visitors.add("user-#{id % 12_000}") }

visitors.estimate       # => 12_361.2
visitors.standard_error # => 0.01625
```

## The native extension is required

There is no pure-Ruby fallback and no `DISABLE_*` switch. HyperLogLog, Count-Min
Sketch, t-digest, cuckoo hashing and Space-Saving are five algorithms, and a
second implementation would mean maintaining each of them twice, with two sets
of accuracy tests and two byte formats to keep in step. The gem is a binding to
the `probabilistic-machines` crate in `ext/probabilistic_machines_native/core`,
and it needs Ruby 4.0 and a Rust toolchain (or a platform gem).

## Sizes are a fixed menu

The crate sizes every structure by a const generic, so each size is a separately
compiled type. The extension carries these and raises `ArgumentError` for
anything else:

| Class            | Keyword              | Values                        |
| ---------------- | -------------------- | ----------------------------- |
| `HyperLogLog`    | `registers:`         | 1024, 4096, 16384, 65536      |
| `CountMinSketch` | `width:` by `depth:` | 1024, 4096 by 4 or 8          |
| `CuckooFilter`   | `buckets:`           | 1024, 4096, 16384, 65536      |
| `HeavyHitters`   | `k:`                 | 16, 64, 256                   |
| `TDigest`        | `compression:`       | any, it is a runtime argument |

```ruby
ProbabilisticMachines::HyperLogLog.new(registers: 3000)
# ArgumentError: registers 3000 is not supported, expected one of [1024, 4096, 16384, 65536]
```

## How many distinct

```ruby
hll = ProbabilisticMachines::HyperLogLog.new(registers: 4096)
hll.add('user-1')
hll.add_hash(0x9e37_79b9_7f4a_7c15) # for streams that ship hashes, not keys
hll.estimate                        # Float
hll.standard_error                  # 1.04 / sqrt(registers)
hll.merge(other)                    # exact union
```

Memory is one byte per register, so 4096 registers cost 4 KiB whether the stream
held ten ids or ten billion.

## How often each

```ruby
cms = ProbabilisticMachines::CountMinSketch.new(width: 4096, depth: 4)
cms.add('/login', count: 3)
cms.count('/login')  # never below the truth
cms.total            # every count added
cms.merge(other)     # exact sum
```

A count is over the truth by at most `e * total / width`, with probability
`1 - e ** -depth` of staying inside it. Width buys accuracy, depth buys
confidence.

## Quantiles

```ruby
td = ProbabilisticMachines::TDigest.new(compression: 100)
td.add(12.5)
td.add(12.5, weight: 1_000.0) # one point standing for a thousand observations
td.quantile(0.99)
td.cdf(12.5)
td.centroids                  # Array of frozen Centroid(mean:, weight:)
td.minimum                    # exact, as is maximum
td.merge(other)
```

A quantile is off by about `1 / compression` of the distribution in the middle
and is exact at the extremes, in `compression * ln(n)` clusters rather than `n`
samples. An empty digest answers `Float::NAN`.

## Have I seen this

```ruby
cf = ProbabilisticMachines::CuckooFilter.new(buckets: 4096)
cf.insert('device-9')      # raises ProbabilisticMachines::Full when wedged
cf.include?('device-9')    # contains? is the same question
cf.remove('device-9')      # unlike a Bloom filter, it can forget
```

`buckets * 4` items in `buckets * 4` bytes. Never a false negative; about 3%
false positives. Only remove items you inserted, or you take a colliding item's
fingerprint with it. A cuckoo filter cannot merge, so there is no `merge` here.

## Who dominates

```ruby
hh = ProbabilisticMachines::HeavyHitters.new(k: 64)
hh.add('10.0.0.1')
hh.add('10.0.0.1', count: 5)
hh.top(10)   # [Hitter(key:, count:, overestimate:), ...] most frequent first
hh.ranked    # every tracked key
hh.observed  # observations fed in
hh.merge(other)
```

After `n` observations every key seen more than `n / k` times is tracked, in `k`
counters and no heap per key. A counter can inherit the count of the key it
evicted, or what a merged-in table's own evictions may have hidden, so the true
count lies between `count - overestimate` and `count`.

Keys are UTF-8 strings here: Space-Saving needs `Copy + Eq` keys in Rust, so the
table counts 64-bit key hashes and the extension keeps the string each tracked
hash came from, to hand back from `top` and `ranked`. The other four classes
take any byte string.

## Serialisation

`dump` returns the crate's own byte frame, `load` reads one back. The frame
names its own shape, so `load` needs no keyword and rejects a blob written with
a size this build does not carry.

```ruby
blob = hll.dump                                 # binary String
ProbabilisticMachines::HyperLogLog.load(blob)
ProbabilisticMachines::TDigest.load(blob)       # ArgumentError: codec: wrong structure tag
```

Anything that reads the crate reads these frames, so an embedded device can ship
a sketch a Rails app folds in. `HeavyHitters` is the one blob that is not purely
the crate's: the crate's frame counts 64-bit key hashes, so the extension appends
its key table after it, one record of a 64-bit byte length and that many UTF-8
bytes per tracked key. `load` rehashes each name, and rejects a blob whose key
table is malformed or does not name every key the frame tracks.

```ruby
restored = ProbabilisticMachines::HeavyHitters.load(hh.dump)
restored.top(1).first.key # => '10.0.0.1', the string, not its hash
```

## Rust

The sketches themselves are the `probabilistic-machines` crate under
`ext/probabilistic_machines_native/core`: `no_std`, zero dependencies, allocating
only for `TDigest` and for the `encoded` frames this extension dumps, and
verified on bare metal. The binding crate beside it under
`ffi/` is the Ruby surface and is not published.

## License

MIT.
