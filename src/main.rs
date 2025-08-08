use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Host / device / config
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No default output device available");
    let supported_config = device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let config: StreamConfig = supported_config.into();
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    // Shared synth state
    let state = Arc::new(Mutex::new(FmSynthState {
        carrier_phase: 0.0,
        mod_phase: 0.0,
        carrier_freq: 220.0,
        mod_freq: 110.0,
        mod_index: 5.0,
        sample_rate,
    }));

    // Build the correct stream for the device's sample format
    let stream = match sample_format {
        SampleFormat::F32 => {
            build_output_stream_f32(&device, &config, channels, Arc::clone(&state))?
        }
        SampleFormat::I16 => {
            build_output_stream_i16(&device, &config, channels, Arc::clone(&state))?
        }
        SampleFormat::U16 => {
            build_output_stream_u16(&device, &config, channels, Arc::clone(&state))?
        }
        _ => panic!("Unsupported sample format"),
    };

    stream.play()?;
    println!("FM Synth running — press Ctrl+C to stop.");

    // Keep the program alive while audio plays
    loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}

struct FmSynthState {
    carrier_phase: f32,
    mod_phase: f32,
    carrier_freq: f32,
    mod_freq: f32,
    mod_index: f32,
    sample_rate: f32,
}

/// Stream builder for f32 output
fn build_output_stream_f32(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    state: Arc<Mutex<FmSynthState>>,
) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let err_fn = |err| eprintln!("Stream error: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
            let mut s = state.lock().unwrap();

            for frame in data.chunks_mut(channels) {
                // compute increments
                let carrier_inc = 2.0 * PI * s.carrier_freq / s.sample_rate;
                let mod_inc = 2.0 * PI * s.mod_freq / s.sample_rate;

                // advance phases
                s.carrier_phase = (s.carrier_phase + carrier_inc) % (2.0 * PI);
                s.mod_phase = (s.mod_phase + mod_inc) % (2.0 * PI);

                // FM: phase modulation
                let mod_signal = s.mod_phase.sin();
                let phase = s.carrier_phase + s.mod_index * mod_signal;
                let sample_val = phase.sin();

                // write to all channels
                for out in frame.iter_mut() {
                    *out = sample_val;
                }
            }
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

/// Stream builder for i16 output
fn build_output_stream_i16(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    state: Arc<Mutex<FmSynthState>>,
) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let err_fn = |err| eprintln!("Stream error: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [i16], _info: &cpal::OutputCallbackInfo| {
            let mut s = state.lock().unwrap();

            for frame in data.chunks_mut(channels) {
                let carrier_inc = 2.0 * PI * s.carrier_freq / s.sample_rate;
                let mod_inc = 2.0 * PI * s.mod_freq / s.sample_rate;

                s.carrier_phase = (s.carrier_phase + carrier_inc) % (2.0 * PI);
                s.mod_phase = (s.mod_phase + mod_inc) % (2.0 * PI);

                let mod_signal = s.mod_phase.sin();
                let phase = s.carrier_phase + s.mod_index * mod_signal;
                let sample_val = phase.sin();

                // Convert f32 [-1.0, 1.0] -> i16 range
                let scaled = (sample_val.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;

                for out in frame.iter_mut() {
                    *out = scaled;
                }
            }
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

/// Stream builder for u16 output
fn build_output_stream_u16(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    state: Arc<Mutex<FmSynthState>>,
) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let err_fn = |err| eprintln!("Stream error: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [u16], _info: &cpal::OutputCallbackInfo| {
            let mut s = state.lock().unwrap();

            for frame in data.chunks_mut(channels) {
                let carrier_inc = 2.0 * PI * s.carrier_freq / s.sample_rate;
                let mod_inc = 2.0 * PI * s.mod_freq / s.sample_rate;

                s.carrier_phase = (s.carrier_phase + carrier_inc) % (2.0 * PI);
                s.mod_phase = (s.mod_phase + mod_inc) % (2.0 * PI);

                let mod_signal = s.mod_phase.sin();
                let phase = s.carrier_phase + s.mod_index * mod_signal;
                let sample_val = phase.sin();

                // Convert f32 [-1.0, 1.0] -> u16 [0, u16::MAX]
                let scaled =
                    (((sample_val.clamp(-1.0, 1.0) * 0.5) + 0.5) * u16::MAX as f32).round() as u16;

                for out in frame.iter_mut() {
                    *out = scaled;
                }
            }
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}
