# Changelog

## [0.3.0](https://github.com/seuros/probabilistic_machines/compare/probabilistic-machines-v0.2.0...probabilistic-machines-v0.3.0) (2026-11-01)


### Features

* accept any cloneable key in HeavyHitters ([108ed8c](https://github.com/seuros/probabilistic_machines/commit/108ed8c79adeffeef708aa298baa7cc64d55d12b))

## [0.2.0](https://github.com/seuros/probabilistic_machines/compare/probabilistic-machines-v0.1.0...probabilistic-machines-v0.2.0) (2026-11-01)


### Features

* HeavyHitters codec and an infallible exact-size encode ([1cf9442](https://github.com/seuros/probabilistic_machines/commit/1cf944208e191f1cc7847221239204e14f0ad9a9))
* probabilistic data structures in no_std Rust ([e8c9711](https://github.com/seuros/probabilistic_machines/commit/e8c97117a19e39ecfb2eff588724d9bd3d33be48))


### Bug Fixes

* a zero cluster budget spun the t-digest fold loop forever ([8f4f146](https://github.com/seuros/probabilistic_machines/commit/8f4f1466ca579a943d8a23aec4075b53314e3824))
* expose and persist count-min saturation ([0ac83ac](https://github.com/seuros/probabilistic_machines/commit/0ac83ac77d02ddee5d7614d73e95e5d103e823e8))
* fold t-digest clusters without overflowing their span ([2e33539](https://github.com/seuros/probabilistic_machines/commit/2e33539b0dbcc0eb8d288eedc936c0ece8d7f7bd))
* interpolate a subnormal t-digest span without underflowing it ([1b05f4e](https://github.com/seuros/probabilistic_machines/commit/1b05f4e5515fa4468730bec3708daf8f57052726))
* interpolate the t-digest cdf without overflowing the span ([48cd8a1](https://github.com/seuros/probabilistic_machines/commit/48cd8a11f1b2fdd56403af46614fd6a56b8b8309))
* keep t-digest total weight finite and the cluster count bounded ([0a37d4b](https://github.com/seuros/probabilistic_machines/commit/0a37d4b52cd5a0abd7de862fe0f52e4f987c956e))
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
