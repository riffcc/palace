//! Audio output for GBA emulation using tinyaudio.

use crate::error::{GbaError, GbaResult};
use rustboyadvance_core::prelude::SimpleAudioInterface;
use rustboyadvance_utils::audio::SampleConsumer;
use rustboyadvance_utils::Consumer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tinyaudio::prelude::*;
use tracing::debug;

/// Sample rate for GBA audio (44.1 kHz).
pub const SAMPLE_RATE: u32 = 44100;

/// Audio player that consumes samples from the emulator.
pub struct AudioPlayer {
    _device: Box<dyn BaseAudioOutputDevice>,
    running: Arc<AtomicBool>,
}

impl AudioPlayer {
    /// Create a new audio player with the given sample consumer.
    pub fn new(mut consumer: SampleConsumer) -> GbaResult<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let params = OutputDeviceParameters {
            channels_count: 2,
            sample_rate: SAMPLE_RATE as usize,
            channel_sample_count: 1024,
        };

        let device = run_output_device(params, move |data| {
            if !running_clone.load(Ordering::Relaxed) {
                data.fill(0.0);
                return;
            }

            for frame in data.chunks_mut(2) {
                if let Some(left) = consumer.try_pop() {
                    if let Some(right) = consumer.try_pop() {
                        frame[0] = left as f32 / 32768.0;
                        frame[1] = right as f32 / 32768.0;
                    } else {
                        frame[0] = left as f32 / 32768.0;
                        frame[1] = frame[0];
                    }
                } else {
                    frame[0] = 0.0;
                    frame[1] = 0.0;
                }
            }
        })
        .map_err(|e| GbaError::Audio(format!("Failed to create audio device: {:?}", e)))?;

        debug!("Audio player initialized at {}Hz", SAMPLE_RATE);

        Ok(Self {
            _device: device,
            running,
        })
    }

    /// Stop audio playback.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Create an audio interface and player for the emulator.
/// Returns (audio_interface, audio_player) - pass the interface to GameBoyAdvance::new().
pub fn create_audio() -> GbaResult<(Box<SimpleAudioInterface>, AudioPlayer)> {
    let (interface, consumer) = SimpleAudioInterface::create_channel(SAMPLE_RATE as i32, None);
    let player = AudioPlayer::new(consumer)?;
    Ok((interface, player))
}
