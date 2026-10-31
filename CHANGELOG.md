# Changelog

## 0.1.0

- Initial release.
- `ProbabilisticMachines::HyperLogLog`: distinct counts in 1024, 4096, 16384 or
  65536 registers, with `merge` for union and `standard_error` for the bound.
- `ProbabilisticMachines::CountMinSketch`: per-item frequencies in a width of
  1024, 4096 or 16384 by a depth of 4 or 8.
- `ProbabilisticMachines::TDigest`: quantiles and CDF from clusters, with a
  runtime compression.
- `ProbabilisticMachines::CuckooFilter`: membership that can forget, in 1024,
  4096, 16384 or 65536 buckets, raising `ProbabilisticMachines::Full` when a
  fingerprint has nowhere to go.
- `ProbabilisticMachines::HeavyHitters`: Space-Saving top-k over 16, 64 or 256
  counters, returning `Hitter` objects that carry the overestimate.
- `dump`/`load` round-trip every sketch through the byte format of the
  `probabilistic-machines` crate. A `HeavyHitters` blob is the crate's frame
  followed by the extension's key table, so the tracked strings survive a
  restart instead of loading back as hashes.
- RBS signatures for the whole public API.
- The `probabilistic-machines` crate codes `HeavyHitters<u64, K>`, and with
  `alloc` hands every structure's frame back from `encoded` instead of filling
  a caller's buffer, with `decode_into` to read one back in place.
