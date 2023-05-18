use std::error::Error;
use std::cmp;
use error::InvalidSizeError;
use misc::*;

#[cfg(not(feature = "rlibc"))]
use std::io::Write;

#[cfg(feature = "rlibc")]
use rlibc;


/// A fast "binned" waveform renderer.
///
/// Minimum / maximum amplitude values are binned to reduce
/// calculation and memory usage.
pub struct BinnedWaveformRenderer<T: Sample> {
    pub config: WaveformConfig,
    sample_rate: f64,
    bin_size: usize,
    minmax: MinMaxPairSequence<T>,
}

impl<T: Sample> BinnedWaveformRenderer<T> {
    /// The constructor.
    ///
    /// # Arguments
    ///
    /// * `samples` - The samples that will be used to calculate binned min / max values.
    ///               It must also contain the sample rate that is used by
    ///               `BinnedWaveformRenderer` to render images when given a
    ///               `TimeRange::Seconds`.
    /// * `bin_size` - The size of the bins which the min / max values will be binned
    ///                into.
    /// * `config` - See `WaveformConfig`.
    pub fn new(samples: &SampleSequence<T>, bin_size: usize, config: WaveformConfig) -> Result<BinnedWaveformRenderer<T>, Box<dyn Error>> {
        let mut data: Vec<MinMaxPair<T>> = Vec::new();
        let nb_samples = samples.data.len();

        if bin_size > nb_samples {
            return Err(Box::new(InvalidSizeError {
                var_name: "bin_size".to_string(),
            }));
        }

        let nb_bins = (nb_samples as f64 / bin_size as f64).ceil() as usize;

        for x in 0..nb_bins {
            let mut min = samples.data[x * bin_size + 0];
            let mut max = samples.data[x * bin_size + 0];
            if bin_size > 1 {
                for i in 1..bin_size {
                    let idx = x * bin_size + i;
                    if idx >= nb_samples {
                        break;
                    }
                    let s = samples.data[idx];
                    if s > max {
                        max = s;
                    } else if s < min {
                        min = s;
                    }
                }
            }
            data.push(MinMaxPair { min: min, max: max });
        }
        let minmax = MinMaxPairSequence { data: data };
        Ok(Self {
            config: config,
            bin_size: bin_size,
            minmax: minmax,
            sample_rate: samples.sample_rate,
        })
    }


    /// Renders an image as a `Vec<u8>`.
    ///
    /// `None` will be returned if the area of the specified `shape` is equal to zero.
    ///
    /// # Arguments
    ///
    /// * `range` - The samples within this `TimeRange` will be rendered.
    /// * `shape` - The `(width, height)` of the resulting image in pixels.
    pub fn render_vec(&self, range: TimeRange, shape: (usize, usize)) -> Option<Vec<u8>> {
        let (w, h) = shape;
        if w == 0 || h == 0 {
            return None;
        }


        let mut img = match self.config.get_background() {
            Color::Scalar(_) => vec![0u8; w * h],
            Color::Vector3{..} => vec![0u8; w * h * 3],
            Color::Vector4{..} => vec![0u8; w * h * 4],
        };
        
        self.render_write(range, (0, 0), shape, &mut img[..], shape).unwrap();

        Some(img)
    }

    /// Writes the image into a mutable reference to a slice.
    ///
    /// It will raise an error if
    ///
    /// * the area of the specified `shape` is equal to zero.
    /// * either the width or height of the `shape` exceeds that of the `full_shape`
    ///   of `img`.
    /// * the length of `img` is not long enough to contain the result.
    ///   `(offsets.0 + shape.0) * (offsets.1 + shape.1) * (Bytes per pixel) <= img.len()`
    ///   must be satisfied.
    ///
    /// # Arguments
    ///
    /// * `range` - The samples within this `TimeRange` will be rendered.
    /// * `offsets` - The `(x-offset, y-offset)` of the part of the `img` that is
    ///               going to be overwritten in in pixels.
    ///               Specifies the starting position to write into `img`.
    /// * `shape` - The `(width, height)` of the part of the `img` that is going 
    ///             to be overwritten in pixels.
    /// * `img`   - A mutable reference to the slice to write the result into.
    /// * `full_shape` - The `(width, height)` of the whole `img` in pixels.
    ///
    pub fn render_write(&self, range: TimeRange, offsets: (usize, usize), shape: (usize, usize), img: &mut [u8], full_shape: (usize, usize)) -> Result<(), Box<dyn Error>> {
        let (w, h) = shape;
        if w == 0 || h == 0 {
            return Err(Box::new(InvalidSizeError{var_name: "shape".to_string()}));
        }

        let (fullw, fullh) = full_shape;
        if fullw < w || fullh < h {
            return Err(Box::new(InvalidSizeError{var_name: "shape and/or full_shape".to_string()}));
        }

        let (offx, offy) = offsets;

        // Check if we have enough bytes in `img`
        match self.config.get_background() {
            Color::Scalar(_) => {
                if (offx + w) * (offy + h) > img.len() {
                    return Err(Box::new(InvalidSizeError{var_name: "offsets and/or shape".to_string()}));
                }
            },
            Color::Vector3{..} => {
                if (offx + w) * (offy + h) * 3 > img.len() {
                    return Err(Box::new(InvalidSizeError{var_name: "offsets and/or shape".to_string()}));
                }
            },
            Color::Vector4{..} => {
                if (offx + w) * (offy + h) * 4 > img.len() {
                    return Err(Box::new(InvalidSizeError{var_name: "offsets and/or shape".to_string()}));
                }
            },
        }

        let (begin, end) = match range {
            TimeRange::Seconds(b, e) => (
                (b * self.sample_rate) as usize,
                (e * self.sample_rate) as usize,
            ),
            TimeRange::Samples(b, e) => (b, e),
        };
        let nb_samples = end - begin;
        let samples_per_pixel = (nb_samples as f64) / (w as f64);
        let bins_per_pixel = samples_per_pixel / (self.bin_size as f64);
        let bins_per_pixel_floor = bins_per_pixel.floor() as usize;
        let bins_per_pixel_ceil = bins_per_pixel.ceil() as usize;

        let offset_bin_idx = begin / self.bin_size;
        let mut start_bin_idx = offset_bin_idx;
        for x in 0..w {
            let inc = if ((start_bin_idx - offset_bin_idx) as f64 + 1f64) / (x as f64) < bins_per_pixel {
                bins_per_pixel_ceil
            } else {
                bins_per_pixel_floor
            };

            let mut min: T;
            let mut max: T;
            if start_bin_idx < self.minmax.data.len() - 1 {
                let ref d = self.minmax.data[start_bin_idx];
                min = d.min;
                max = d.max;
                let range_start = start_bin_idx;
                let range_end = if start_bin_idx + inc <= self.minmax.data.len() {
                    start_bin_idx + inc
                } else {
                    self.minmax.data.len()
                };
                for b in self.minmax.data[range_start..range_end].iter() {
                    if b.min < min {
                        min = b.min
                    }
                    if b.max > max {
                        max = b.max
                    }
                }
                start_bin_idx = range_end;
            } else {
                min = T::zero();
                max = T::zero();
            }

            let scale = 1f64 / (self.config.amp_max - self.config.amp_min) * (h as f64);
            let min_translated: usize = h -
                cmp::max(
                    0,
                    cmp::min(
                        h as i32,
                        ((min.into() - self.config.amp_min) * scale).floor() as i32,
                    ),
                ) as usize;
            let max_translated: usize = h -
                cmp::max(
                    0,
                    cmp::min(
                        h as i32,
                        ((max.into() - self.config.amp_min) * scale).floor() as i32,
                    ),
                ) as usize;

            // Putting this `match` outside for loops improved the speed.
            match (self.config.get_background(), self.config.get_foreground()) {
                (Color::Scalar(ba), Color::Scalar(fa)) => {
                    flipping_three_segment_for!{
                                for y in 0, max_translated, min_translated, h, {
                                    pixel!(img[fullw, fullh; offx+x, offy+y]) = ba,
                                    pixel!(img[fullw, fullh; offx+x, offy+y]) = fa
                                }
                            }
                },

                (
                    Color::Vector3 (br, bg, bb),
                    Color::Vector3 (fr, fg, fb),
                ) => {
                    // Order the RGB values so we can directly
                    // copy them into the image.
                    let bg_colors: [u8; 3] = [br, bg, bb];
                    let fg_colors: [u8; 3] = [fr, fg, fb];

                    // Each `flipping_three_segment_for` macro
                    // will be expanded into three for loops below.
                    //
                    // I could have used just one for loop (and I did once)
                    // but this made a significant difference in
                    // the performance.
                    //
                    // The `pixel` macro is used to access pixels.
                    //
                    // See src/macros/*.rs for the defenitions.


                    #[cfg(feature = "rlibc")]
                    unsafe {
                        flipping_three_segment_for!{
                                for y in 0, max_translated, min_translated, h, {
                                        rlibc::memcpy(
                                            &mut pixel!(img[fullw, fullh, 3; offx+x, offy+y, 0]) as _,
                                            &bg_colors[0] as _,
                                            3
                                            ),
                                        rlibc::memcpy(
                                            &mut pixel!(img[fullw, fullh, 3; offx+x, offy+y, 0]) as _,
                                            &fg_colors[0] as _,
                                            3
                                            )
                                }
                            }
                    }

                    // A similar implementation is possible without
                    // the rlibc crate, but it appeared to be
                    // slightly slower.
                    #[cfg(not(feature = "rlibc"))]
                    {
                        flipping_three_segment_for!{
                                for y in 0, max_translated, min_translated, h, {
                                    (&mut pixel!(img[fullw, fullh, 3; offx+x, offy+y, 0 => 4]))
                                        .write(&bg_colors).unwrap(),
                                    (&mut pixel!(img[fullw, fullh, 3; offx+x, offy+y, 0 => 4]))
                                        .write(&fg_colors).unwrap()
                                }
                            }
                    }

                },

                (
                    Color::Vector4 (br, bg, bb, ba),
                    Color::Vector4 (fr, fg, fb, fa),
                ) => {

                    // Order the RGBA values so we can directly
                    // copy them into the image.
                    let bg_colors: [u8; 4] = [br, bg, bb, ba];
                    let fg_colors: [u8; 4] = [fr, fg, fb, fa];

                    // Each `flipping_three_segment_for` macro
                    // will be expanded into three for loops below.
                    //
                    // I could have used just one for loop (and I did once)
                    // but this made a significant difference in
                    // the performance.
                    //
                    // The `pixel` macro is used to access pixels.
                    //
                    // See src/macros/*.rs for the defenitions.


                    #[cfg(feature = "rlibc")]
                    unsafe {
                        flipping_three_segment_for!{
                                for y in 0, max_translated, min_translated, h, {
                                        rlibc::memcpy(
                                            &mut pixel!(img[fullw, fullh, 4; offx+x, offy+y, 0]) as _,
                                            &bg_colors[0] as _,
                                            4
                                            ),
                                        rlibc::memcpy(
                                            &mut pixel!(img[fullw, fullh, 4; offx+x, offy+y, 0]) as _,
                                            &fg_colors[0] as _,
                                            4
                                            )
                                }
                            }
                    }

                    // A similar implementation is possible without
                    // the rlibc crate, but it appeared to be
                    // slightly slower.
                    #[cfg(not(feature = "rlibc"))]
                    {
                        flipping_three_segment_for!{
                                for y in 0, max_translated, min_translated, h, {
                                    (&mut pixel!(img[fullw, fullh, 4; offx+x, offy+y, 0 => 4]))
                                        .write(&bg_colors).unwrap(),
                                    (&mut pixel!(img[fullw, fullh, 4; offx+x, offy+y, 0 => 4]))
                                        .write(&fg_colors).unwrap()
                                }
                            }
                    }
                },

                // This case is unreachable because inconsistent
                // `Color` formats are checked whenever a user
                // creates a `WaveformConfig`.
                (_, _) => unreachable!(), 
            }
        }

        Ok(())
    }

    pub fn get_bin_size(&self) -> usize {
        self.bin_size
    }
    pub fn get_sample_rate(&self) -> f64 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::BinnedWaveformRenderer;
    use ::misc::*;

    #[test]
    fn render_vec_and_write_eq() {
        let tr = TimeRange::Seconds(0f64, 10f64);
        let (width, height) = (1000, 100);
        let mut samples: Vec<f64> = Vec::new();
        for t in 0u32..44100u32 {
            samples.push(((t as f64) * 0.01f64 * 2f64 * 3.1415f64).sin());
        }
        let config = WaveformConfig::new(
            -1f64,
            1f64,
            Color::Vector4(0, 0, 0, 255),
            Color::Vector4(0, 0, 0, 255)
            ).unwrap();
        let wfr = BinnedWaveformRenderer::new(
            &SampleSequence {
                data: &samples[..],
                sample_rate: 44100f64,
            },
            10,
            config,
        ).unwrap();

        let v1 = wfr.render_vec(tr, (width, height)).unwrap();

        let mut v2: Vec<u8> = vec![0; width*height*4];

        wfr.render_write(tr, (0, 0), (width, height), &mut v2[..], (width, height)).unwrap();

        assert_eq!(v1, v2);
    }

    #[test]
    fn markers() {
        let c = Color::Scalar(0);
        let config = WaveformConfig::new(-1f64, 1f64, c, c).unwrap();
        let wfr = BinnedWaveformRenderer::new(
            &SampleSequence {
                data: &vec![0f64; 10],
                sample_rate: 44100f64,
            },
            10, config
        ).unwrap();
        let _test: &(Sync+Send) = &wfr;
    }
}
