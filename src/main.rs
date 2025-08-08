use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

struct SineOsc {
    phase: f32,
    phase_inc: f32,
}

impl SineOsc {
    fn new(freq: f32, sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            phase_inc: freq * TAU / sample_rate,
        }
    }
    fn set_freq(&mut self, freq: f32, sample_rate: f32) {
        self.phase_inc = freq * TAU / sample_rate;
    }
    fn next(&mut self) -> f32 {
        let v = self.phase.sin();
        self.phase += self.phase_inc;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        v
    }
}

#[derive(Clone, Copy)]
struct Adsr {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    state: AdsrState,
    level: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum AdsrState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Adsr {
    fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            attack: attack.max(1e-6),
            decay: decay.max(1e-6),
            sustain,
            release: release.max(1e-6),
            state: AdsrState::Idle,
            level: 0.0,
        }
    }

    fn note_on(&mut self) {
        self.state = AdsrState::Attack;
    }

    fn note_off(&mut self) {
        if self.state != AdsrState::Idle {
            self.state = AdsrState::Release;
        }
    }

    fn next(&mut self, dt: f32) -> f32 {
        match self.state {
            AdsrState::Idle => {
                self.level = 0.0;
            }
            AdsrState::Attack => {
                self.level += dt / self.attack;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.state = AdsrState::Decay;
                }
            }
            AdsrState::Decay => {
                self.level -= dt / self.decay * (1.0 - self.sustain);
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.state = AdsrState::Sustain;
                }
            }
            AdsrState::Sustain => {
                self.level = self.sustain;
            }
            AdsrState::Release => {
                // scaled by current level to avoid weird jumps
                self.level -= dt / self.release * (self.level.max(1e-6));
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.state = AdsrState::Idle;
                }
            }
        }
        self.level
    }
}

#[derive(Clone)]
struct SynthState {
    carrier_freq: f32,
    mod_ratio: f32,
    mod_index: f32,
    amp: f32,
    adsr: Adsr,
    gate: bool,
}

fn main() -> Result<(), anyhow::Error> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No output device available");
    println!("Output device: {}", device.name()?);

    let config = device.default_output_config()?.config();
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    println!("Sample rate: {}, channels: {}", sample_rate, channels);

    // initial synth state
    let state = SynthState {
        carrier_freq: 220.0,
        mod_ratio: 2.0,
        mod_index: 100.0,
        amp: 0.2,
        adsr: Adsr::new(0.01, 0.1, 0.8, 0.3),
        gate: false,
    };

    let shared = Arc::new(Mutex::new(state));
    let shared_ui = shared.clone();

    // Create oscillators local to the callback, but we construct them here so their memory lives long.
    // We'll use move closure capturing arcs.
    let channels_copy = channels;
    let sample_rate_copy = sample_rate;

    let err_fn = |err| eprintln!("Stream error: {}", err);

    // Build stream depending on sample format
    let stream = match device.default_output_config()?.sample_format {
        cpal::SampleFormat::F32 => build_and_run_stream::<f32>(
            &device,
            &config,
            shared.clone(),
            channels_copy,
            sample_rate_copy,
            err_fn,
        )?,
        cpal::SampleFormat::I16 => build_and_run_stream::<i16>(
            &device,
            &config,
            shared.clone(),
            channels_copy,
            sample_rate_copy,
            err_fn,
        )?,
        cpal::SampleFormat::U16 => build_and_run_stream::<u16>(
            &device,
            &config,
            shared.clone(),
            channels_copy,
            sample_rate_copy,
            err_fn,
        )?,
    };

    stream.play()?;

    println!("FM synth running. Type commands (q to quit).");
    println!("Commands: n <hz>, r <ratio>, i <index>, a <amp>, on, off");

    use std::io::{self, BufRead};
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let l = line.unwrap_or_default();
        let mut parts = l.trim().split_whitespace();
        if let Some(cmd) = parts.next() {
            match cmd {
                "q" | "quit" => break,
                "n" => {
                    if let Some(s) = parts.next() {
                        if let Ok(freq) = s.parse::<f32>() {
                            let mut s = shared_ui.lock().unwrap();
                            s.carrier_freq = freq.max(1.0);
                            println!("Carrier freq = {}", s.carrier_freq);
                        }
                    }
                }
                "r" => {
                    if let Some(s) = parts.next() {
                        if let Ok(v) = s.parse::<f32>() {
                            let mut s = shared_ui.lock().unwrap();
                            s.mod_ratio = v.max(0.0);
                            println!("Mod ratio = {}", s.mod_ratio);
                        }
                    }
                }
                "i" => {
                    if let Some(s) = parts.next() {
                        if let Ok(v) = s.parse::<f32>() {
                            let mut s = shared_ui.lock().unwrap();
                            s.mod_index = v.max(0.0);
                            println!("Mod index = {}", s.mod_index);
                        }
                    }
                }
                "a" => {
                    if let Some(s) = parts.next() {
                        if let Ok(v) = s.parse::<f32>() {
                            let mut s = shared_ui.lock().unwrap();
                            s.amp = v.clamp(0.0, 1.0);
                            println!("Amplitude = {}", s.amp);
                        }
                    }
                }
                "on" => {
                    let mut s = shared_ui.lock().unwrap();
                    s.gate = true;
                    s.adsr.note_on();
                    println!("Note ON");
                }
                "off" => {
                    let mut s = shared_ui.lock().unwrap();
                    s.gate = false;
                    s.adsr.note_off();
                    println!("Note OFF");
                }
                _ => println!("Unknown command"),
            }
        }
        thread::sleep(Duration::from_millis(5));
    }

    Ok(())
}

fn build_and_run_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Mutex<SynthState>>,
    channels: usize,
    sample_rate: f32,
    err_fn: impl Fn(cpal::StreamError) + Send + Sync + 'static,
) -> Result<cpal::Stream, anyhow::Error>
where
    T: cpal::Sample,
{
    // Local oscillators used by the callback
    let mut carrier = SineOsc::new(220.0, sample_rate);
    let mut modulator = SineOsc::new(440.0, sample_rate);

    // time delta per sample
    let dt = 1.0 / sample_rate;

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            // audio callback
            // data length = frames * channels
            let mut idx = 0;
            while idx < data.len() {
                // snapshot state
                let snapshot = {
                    let s = shared.lock().unwrap();
                    s.clone()
                };

                // set oscillator frequencies according to snapshot
                let carrier_freq = snapshot.carrier_freq;
                let mod_freq = snapshot.carrier_freq * snapshot.mod_ratio;

                carrier.set_freq(carrier_freq, sample_rate);
                modulator.set_freq(mod_freq, sample_rate);

                // generate one sample (mono) with FM: carrier phase is modulated
                // instantaneous frequency offset = mod_index * modulator_sample
                // We implement phase modulation via adding to carrier phase increment (approx).
                // Better approach: compute modulator value and add to carrier phase directly:
                let mod_sample = modulator.next();
                // frequency deviation in Hz
                let freq_deviation = snapshot.mod_index * mod_sample;
                // compute instantaneous carrier phase increment
                let inst_phase_inc = (carrier_freq + freq_deviation) * TAU / sample_rate;

                // advance carrier manually using inst_phase_inc
                // (we cheat a bit and override carrier.phase_inc for this sample)
                let prev_inc = carrier.phase_inc;
                carrier.phase_inc = inst_phase_inc;
                let sample = carrier.next();
                carrier.phase_inc = prev_inc; // restore nominal inc (will be set next loop anyway)

                // envelope
                let mut s2 = shared.lock().unwrap();
                // update the ADSR in shared state and get envelope value
                // If gate toggled, ADSR state already set by UI; here we just step it
                let env_level = {
                    let mut adsr_local = s2.adsr;
                    // step envelope dt
                    let level = {
                        let mut ad = adsr_local;
                        ad.next(dt)
                    };
                    // write back updated ADSR state into shared
                    s2.adsr = adsr_local;
                    level
                };

                // final amplitude
                let out = sample * env_level * snapshot.amp;

                // write to all channels (stereo duplicate)
                for ch in 0..channels {
                    data[idx] = cpal::Sample::from::<f32>(&out);
                    idx += 1;
                }
            }
        },
        err_fn,
    )?;

    Ok(stream)
}
