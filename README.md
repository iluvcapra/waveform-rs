# waveform

waveform is a set of (currently two) waveform image renderers written in Rust.

Work is in progress.

This lib currently has no backends, so it directly manipulates `Vec<u8>`s.

## Features

* Generation of bicolored raster images (outputs are `Vec<u8>`)
  * RGBA format images
  * Gray scale (scalar pixel) images for use as masks etc.
* Direct rendering from a Vec of samples
* Faster indirect rendering from cache
* Time-range specification in either seconds (f64) or samples (usize)

## Screenshot

```sh
cargo run --release --example waveform
```

![examples/waveform.rs](https://user-images.githubusercontent.com/29127111/27250722-dd579ff6-5370-11e7-99c2-7dc3e7705c14.png)

