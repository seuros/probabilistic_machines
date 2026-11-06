# Changelog

## [0.1.4](https://github.com/seuros/probabilistic_machines/compare/probabilistic_machines/v0.1.3...probabilistic_machines/v0.1.4) (2026-11-06)


### Bug Fixes

* keep test modules out of the packaged crate ([e3c1a1f](https://github.com/seuros/probabilistic_machines/commit/e3c1a1f7376dbada6efe782c1d4d264273483432))
* share frame encode/decode helpers across sketches ([067d23d](https://github.com/seuros/probabilistic_machines/commit/067d23db55b8b9068fdef445a7f6dee483961a82))

## [0.1.3](https://github.com/seuros/probabilistic_machines/compare/probabilistic_machines/v0.1.2...probabilistic_machines/v0.1.3) (2026-11-01)


### Bug Fixes

* enable the rb-sys compiled fallback so new Rubies can build ([e8a649d](https://github.com/seuros/probabilistic_machines/commit/e8a649d45475c77f0a5d19c5fa1477100da0cc1b))

## [0.1.2](https://github.com/seuros/probabilistic_machines/compare/probabilistic_machines/v0.1.1...probabilistic_machines/v0.1.2) (2026-11-01)


### Features

* accept any cloneable key in HeavyHitters ([108ed8c](https://github.com/seuros/probabilistic_machines/commit/108ed8c79adeffeef708aa298baa7cc64d55d12b))
* pass a rubygems OTP through the environment ([7a6cc44](https://github.com/seuros/probabilistic_machines/commit/7a6cc446dc737f44e46c4e41b0a76d1fc9b7f5cc))


### Bug Fixes

* a republished platform gem is skipped, not fatal ([6210171](https://github.com/seuros/probabilistic_machines/commit/6210171a4ae679293d9cc6cfbdd82a9d99611a6e))
* collect the built gems whatever depth the download puts them at ([b377177](https://github.com/seuros/probabilistic_machines/commit/b377177879e2a922184eb3e5f9965e8f8c806f00))
* the release tag carries the gem name, and the gate uses the aggregate output ([9b949b4](https://github.com/seuros/probabilistic_machines/commit/9b949b4dc257b4251ff7c8806d6c8efdb05ac26d))

## [0.1.1](https://github.com/seuros/probabilistic_machines/compare/probabilistic_machines-v0.1.0...probabilistic_machines/v0.1.1) (2026-11-01)


### Features

* HeavyHitters codec and an infallible exact-size encode ([1cf9442](https://github.com/seuros/probabilistic_machines/commit/1cf944208e191f1cc7847221239204e14f0ad9a9))
* probabilistic data structures in no_std Rust ([e8c9711](https://github.com/seuros/probabilistic_machines/commit/e8c97117a19e39ecfb2eff588724d9bd3d33be48))
* Ruby gem and magnus FFI for the five structures ([83144bd](https://github.com/seuros/probabilistic_machines/commit/83144bd4553f2ea51fd8437ab966142b50122cec))


### Bug Fixes

* a loaded t-digest honours the compression its constructor requires ([dae877e](https://github.com/seuros/probabilistic_machines/commit/dae877eb1161dfc16f5238329c8c6fdd4b1c78e5))
* a source install can build, and a failed gem push fails the release ([4b18e42](https://github.com/seuros/probabilistic_machines/commit/4b18e42a9b4c7d6264cc3cc9f685e0cb597de409))
* a zero cluster budget spun the t-digest fold loop forever ([8f4f146](https://github.com/seuros/probabilistic_machines/commit/8f4f1466ca579a943d8a23aec4075b53314e3824))
* expose and persist count-min saturation ([0ac83ac](https://github.com/seuros/probabilistic_machines/commit/0ac83ac77d02ddee5d7614d73e95e5d103e823e8))
* fold t-digest clusters without overflowing their span ([2e33539](https://github.com/seuros/probabilistic_machines/commit/2e33539b0dbcc0eb8d288eedc936c0ece8d7f7bd))
* interpolate a subnormal t-digest span without underflowing it ([1b05f4e](https://github.com/seuros/probabilistic_machines/commit/1b05f4e5515fa4468730bec3708daf8f57052726))
* interpolate the t-digest cdf without overflowing the span ([48cd8a1](https://github.com/seuros/probabilistic_machines/commit/48cd8a11f1b2fdd56403af46614fd6a56b8b8309))
* keep t-digest total weight finite and the cluster count bounded ([0a37d4b](https://github.com/seuros/probabilistic_machines/commit/0a37d4b52cd5a0abd7de862fe0f52e4f987c956e))
* refuse a fractional compression too ([379c001](https://github.com/seuros/probabilistic_machines/commit/379c0012d2e5c21131ec8b9fb92534e796997be9))
* refuse a fractional count instead of truncating it to zero ([0b88ae8](https://github.com/seuros/probabilistic_machines/commit/0b88ae8204ebbb761e8aab42df3e7d014501d135))
* reject a compression a t-digest cannot honour ([f53e07b](https://github.com/seuros/probabilistic_machines/commit/f53e07b3af5377565253e14c7327b90c9975eb95))
* reject a hyperloglog rank its precision cannot reach ([c41210b](https://github.com/seuros/probabilistic_machines/commit/c41210b66ccfc305240244fcf151990ea317392e))
* reject a t-digest frame that lost whole clusters ([2e7e0b5](https://github.com/seuros/probabilistic_machines/commit/2e7e0b5594ff14297d98cb5c32380beed1f8192a))
* reject a t-digest frame whose total weight cannot exist ([4bdf21f](https://github.com/seuros/probabilistic_machines/commit/4bdf21fa69949159aa30a8d400a8c7d11e64fbf1))
* reject count-min counters its total cannot explain ([4413fe9](https://github.com/seuros/probabilistic_machines/commit/4413fe965cfdad617f46d9f2f2720e13a3aee6d0))
* reject heavy-hitter records no stream could leave ([3a4172e](https://github.com/seuros/probabilistic_machines/commit/3a4172e730fd7c46b06d39bde2ef2c6034be19d4))
* ship the core's test files so the packaged crate builds ([b567b39](https://github.com/seuros/probabilistic_machines/commit/b567b39144fd403f92efcb7ac48120be228d3b6f))
* t-digest overflow and NaN, and report count-min saturation ([f3aa87c](https://github.com/seuros/probabilistic_machines/commit/f3aa87c018465de0618125b3f2f84e6848e57f37))
* the t-digest arithmetic builds without std ([8009555](https://github.com/seuros/probabilistic_machines/commit/800955549c872ff7db8d257fbe63b18d326c1aa2))
* validate HLL registers on decode, and the same hash collision ([496c077](https://github.com/seuros/probabilistic_machines/commit/496c0771f9a2c06766c1296c7123ea3b6bd7f4ae))
* widen heavy-hitter bounds for what a merged table evicted ([41b0a9c](https://github.com/seuros/probabilistic_machines/commit/41b0a9c33f7433b915f448820628604c107d1cad))

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
