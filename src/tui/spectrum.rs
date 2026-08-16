//! Real-time dynamic audio spectrum analyzer and PCM audio capture pipeline.
//!
//! Captures low-latency PCM audio stream from PipeWire/PulseAudio monitor sink and computes
//! multi-band Fast Fourier Transforms (FFT) for reactive visualizer rendering.

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};
use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// FFT window sample length (approx 46.4ms at 44.1kHz for ~21.5Hz bin resolution).
pub const FFT_SIZE: usize = 2048;

/// Number of discrete frequency bands across the audiophile spectrum (20Hz - 20kHz).
pub const NUM_BANDS: usize = 48;

/// Audio sample rate used for PCM stream capture and frequency analysis.
pub const SAMPLE_RATE: f32 = 44100.0;

/// Min and max frequencies analyzed (in Hz).
const MIN_FREQ: f32 = 20.0;
const MAX_FREQ: f32 = 20000.0;

/// Manages headless PipeWire/PulseAudio capture process and sample buffering.
pub struct AudioCapture {
    samples: Arc<Mutex<VecDeque<f32>>>,
    stop_flag: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
    child: Option<Child>,
    is_active: bool,
}

impl AudioCapture {
    /// Initializes a new audio capture instance (in inactive/stopped state).
    pub fn new() -> Self {
        Self {
            samples: Arc::new(Mutex::new(VecDeque::with_capacity(FFT_SIZE * 4))),
            stop_flag: Arc::new(AtomicBool::new(false)),
            worker_handle: None,
            child: None,
            is_active: false,
        }
    }

    /// Checks if audio capture is currently running.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Starts capturing raw PCM audio from PipeWire/PulseAudio monitor sink in a background worker.
    pub fn start(&mut self) {
        if self.is_active {
            return;
        }

        self.stop();

        self.stop_flag.store(false, Ordering::SeqCst);
        let stop_flag = self.stop_flag.clone();
        let samples_buf = self.samples.clone();

        // 1. Try launching parec targeting the default playback monitor source
        let parec_child = Command::new("parec")
            .arg("--rate=44100")
            .arg("--channels=2")
            .arg("--format=s16le")
            .arg("-d")
            .arg("@DEFAULT_MONITOR@")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        let mut child = match parec_child {
            Ok(c) => c,
            Err(_) => {
                // 2. Fallback to pw-record targeting @DEFAULT_MONITOR@ or default capture
                match Command::new("pw-record")
                    .arg("--raw")
                    .arg("--rate")
                    .arg("44100")
                    .arg("--channels")
                    .arg("2")
                    .arg("--format")
                    .arg("s16")
                    .arg("--target")
                    .arg("@DEFAULT_MONITOR@")
                    .arg("-")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Failed to spawn audio capture (parec / pw-record): {}", e);
                        return;
                    }
                }
            }
        };

        let stdout = match child.stdout.take() {
            Some(out) => out,
            None => {
                let _ = child.kill();
                return;
            }
        };

        self.child = Some(child);
        self.is_active = true;

        // Spawn background reader thread
        let handle = thread::Builder::new()
            .name("reso-audio-capture".to_string())
            .spawn(move || {
                let mut reader = stdout;
                // Buffer for 512 stereo 16-bit frames = 512 * 2 channels * 2 bytes = 2048 bytes
                let mut raw_buf = [0u8; 2048];

                while !stop_flag.load(Ordering::Relaxed) {
                    match reader.read(&mut raw_buf) {
                        Ok(0) => {
                            // EOF or pipe closed
                            break;
                        }
                        Ok(bytes_read) => {
                            let frame_count = bytes_read / 4;
                            if frame_count == 0 {
                                continue;
                            }

                            // Convert 16-bit stereo PCM to normalized mono float [-1.0, 1.0]
                            let mut temp_mono = Vec::with_capacity(frame_count);
                            for i in 0..frame_count {
                                let offset = i * 4;
                                let left =
                                    i16::from_le_bytes([raw_buf[offset], raw_buf[offset + 1]])
                                        as f32;
                                let right =
                                    i16::from_le_bytes([raw_buf[offset + 2], raw_buf[offset + 3]])
                                        as f32;
                                let mono = (left + right) / (2.0 * 32768.0);
                                temp_mono.push(mono);
                            }

                            // Push to circular buffer with lock
                            if let Ok(mut lock) = samples_buf.lock() {
                                for sample in temp_mono {
                                    lock.push_back(sample);
                                }
                                // Keep buffer size bounded to prevent unbounded memory growth
                                while lock.len() > FFT_SIZE * 4 {
                                    lock.pop_front();
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {
                            continue;
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
            })
            .ok();

        self.worker_handle = handle;
    }

    /// Stops audio capture and terminates background worker.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);

        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }

        if let Ok(mut lock) = self.samples.lock() {
            lock.clear();
        }

        self.is_active = false;
    }

    /// Retrieves the most recent window of PCM samples for FFT processing.
    pub fn get_latest_samples(&self, out: &mut [f32; FFT_SIZE]) -> bool {
        if let Ok(lock) = self.samples.lock() {
            let available = lock.len();
            if available < 256 {
                return false;
            }

            let start = available.saturating_sub(FFT_SIZE);
            let slice_len = available - start;
            let pad = FFT_SIZE.saturating_sub(slice_len);

            // Zero-pad if needed
            for val in out.iter_mut().take(pad) {
                *val = 0.0;
            }

            for (idx, sample) in lock.range(start..).enumerate() {
                if pad + idx < FFT_SIZE {
                    out[pad + idx] = *sample;
                }
            }
            true
        } else {
            false
        }
    }
}

impl Default for AudioCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Pre-calculated frequency band index boundaries and loudness compensation weights.
#[derive(Clone, Debug)]
struct BandConfig {
    start_bin: usize,
    end_bin: usize,
    weight: f32,
}

/// High-performance, zero-allocation real-time spectrum analyzer.
pub struct SpectrumAnalyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    hann_window: Vec<f32>,
    bands: Vec<BandConfig>,
    max_history: f32,
    // Pre-allocated scratch buffers to eliminate heap allocations per frame
    input_buf: Vec<f32>,
    spectrum_buf: Vec<Complex32>,
    scratch_buf: Vec<Complex32>,
}

impl SpectrumAnalyzer {
    /// Creates and pre-computes the FFT planner, Hann window, and logarithmic bands.
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);

        // 1. Pre-compute Hann window: w[n] = 0.5 * (1 - cos(2*pi*n / (N - 1)))
        let hann_window: Vec<f32> = (0..FFT_SIZE)
            .map(|n| {
                0.5 * (1.0
                    - ((2.0 * std::f32::consts::PI * n as f32) / (FFT_SIZE as f32 - 1.0)).cos())
            })
            .collect();

        // 2. Pre-compute logarithmic frequency band ranges
        let num_bins = FFT_SIZE / 2 + 1;
        let bin_res = SAMPLE_RATE / FFT_SIZE as f32; // ~21.53 Hz per bin

        let mut bands = Vec::with_capacity(NUM_BANDS);
        for i in 0..NUM_BANDS {
            // Logarithmic frequency scale from 20Hz to 20kHz
            let f_low = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf(i as f32 / NUM_BANDS as f32);
            let f_high = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf((i + 1) as f32 / NUM_BANDS as f32);

            let start_bin = ((f_low / bin_res).floor() as usize)
                .max(1)
                .min(num_bins - 1);
            let end_bin = ((f_high / bin_res).ceil() as usize)
                .max(start_bin + 1)
                .min(num_bins);

            // Center frequency for acoustic weighting
            let f_center = (f_low * f_high).sqrt();

            // Acoustic Equal-Loudness & Equalizer compensation:
            // High frequencies have energy spread across many FFT bins so need higher scaling
            let weight = if f_center < 150.0 {
                // Sub-bass
                1.1
            } else if f_center < 600.0 {
                // Bass & low mids
                1.3
            } else if f_center < 2500.0 {
                // Midrange
                1.8
            } else if f_center < 7000.0 {
                // High-mids & presence
                2.5
            } else {
                // Brilliance / treble
                3.2
            };

            bands.push(BandConfig {
                start_bin,
                end_bin,
                weight,
            });
        }

        let input_buf = fft.make_input_vec();
        let spectrum_buf = fft.make_output_vec();
        let scratch_buf = fft.make_scratch_vec();

        Self {
            fft,
            hann_window,
            bands,
            max_history: 0.08,
            input_buf,
            spectrum_buf,
            scratch_buf,
        }
    }

    /// Computes the normalized frequency band amplitudes [0.0, 1.0] from a PCM sample window.
    pub fn compute_spectrum(&mut self, samples: &[f32; FFT_SIZE], target_bands: &mut [f32]) {
        if target_bands.len() != NUM_BANDS {
            return;
        }

        // Apply Hann window and copy to input buffer
        for (input, (&sample, &win)) in self
            .input_buf
            .iter_mut()
            .zip(samples.iter().zip(&self.hann_window))
        {
            *input = sample * win;
        }

        // Forward Real-to-Complex FFT (zero allocations)
        if self
            .fft
            .process_with_scratch(
                &mut self.input_buf,
                &mut self.spectrum_buf,
                &mut self.scratch_buf,
            )
            .is_err()
        {
            return;
        }

        // Compute magnitude and group into logarithmic frequency bands
        let inv_fft_size = 2.0 / FFT_SIZE as f32;
        let mut frame_peak = 0.0001f32;
        let mut raw_bands = [0.0f32; NUM_BANDS];

        for (band_idx, band) in self.bands.iter().enumerate() {
            let count = (band.end_bin - band.start_bin).max(1) as f32;
            let mut sum_sq = 0.0f32;

            for bin in band.start_bin..band.end_bin {
                if let Some(c) = self.spectrum_buf.get(bin) {
                    let mag = (c.re * c.re + c.im * c.im).sqrt() * inv_fft_size;
                    sum_sq += mag * mag;
                }
            }

            let rms = (sum_sq / count).sqrt() * band.weight;
            raw_bands[band_idx] = rms;
            if rms > frame_peak {
                frame_peak = rms;
            }
        }

        // Adaptive peak AGC (Auto-Gain Control) follower
        if frame_peak > self.max_history {
            self.max_history = self.max_history * 0.6 + frame_peak * 0.4;
        } else {
            self.max_history = (self.max_history * 0.990).max(0.012);
        }

        let ceiling = self.max_history.max(0.015);

        // Normalize each band into [0.0, 1.0] with dynamic compression
        for (i, &raw) in raw_bands.iter().enumerate() {
            if raw < 0.0005 {
                target_bands[i] = 0.0;
                continue;
            }

            let ratio = (raw / ceiling).clamp(0.0, 1.3);
            // Non-linear perceptual compression for lively, dynamic punch
            let visual_amp = (ratio.powf(0.65) * 0.95).clamp(0.0, 1.0);
            target_bands[i] = visual_amp;
        }
    }
}

impl Default for SpectrumAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spectrum_analyzer_silence() {
        let mut analyzer = SpectrumAnalyzer::new();
        let samples = [0.0f32; FFT_SIZE];
        let mut target_bands = [0.0f32; NUM_BANDS];

        analyzer.compute_spectrum(&samples, &mut target_bands);

        for (i, &val) in target_bands.iter().enumerate() {
            assert!(
                val < 0.05,
                "Band {} should be silent/near-zero, got {}",
                i,
                val
            );
        }
    }

    #[test]
    fn test_spectrum_analyzer_bass_detection() {
        let mut analyzer = SpectrumAnalyzer::new();
        let mut samples = [0.0f32; FFT_SIZE];
        let freq = 120.0f32; // 120 Hz Bass tone

        for (n, sample) in samples.iter_mut().enumerate() {
            *sample = (2.0 * std::f32::consts::PI * freq * n as f32 / SAMPLE_RATE).sin() * 0.8;
        }

        let mut target_bands = [0.0f32; NUM_BANDS];
        analyzer.compute_spectrum(&samples, &mut target_bands);

        let max_bass = target_bands[2..12].iter().cloned().fold(0.0f32, f32::max);
        let max_treble = target_bands[35..48].iter().cloned().fold(0.0f32, f32::max);

        assert!(
            max_bass > 0.4,
            "120Hz tone should register strongly in bass bands, got max {}",
            max_bass
        );
        assert!(
            max_treble < 0.1,
            "120Hz tone should not register in high treble bands, got {}",
            max_treble
        );
    }

    #[test]
    fn test_audio_capture_lifecycle() {
        let mut capture = AudioCapture::new();
        assert!(!capture.is_active());
        capture.start();
        capture.stop();
        assert!(!capture.is_active());
        capture.stop();
    }
}
