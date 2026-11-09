// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The mixer's tests that need its insides: the queues, the slots, the
//! arithmetic. The offline tests through the public API are in `tests/`.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::dsp::{CEILING, KNEE, Ramp, attenuation, limit, pan_gains};
use crate::shared::lock;
use crate::voice::{MAX_STEP, ONE, step};
use crate::{AudioConfig, PlayParams, Sound, SoundData};

#[test]
fn a_ramp_is_linear_and_lands_on_its_target() {
    let mut ramp = Ramp::at(1.0);
    ramp.to(0.0, 4);
    let values: Vec<f32> = (0..6).map(|_| ramp.next()).collect();
    assert_eq!(values, [0.75, 0.5, 0.25, 0.0, 0.0, 0.0]);
    assert_eq!((ramp.target(), ramp.remaining()), (0.0, 0));
    // Retargeted halfway, it goes on from where it is.
    ramp.to(1.0, 10);
    for _ in 0..5 {
        ramp.next();
    }
    ramp.to(0.0, 2);
    assert_eq!([ramp.next(), ramp.next()], [0.25, 0.0]);
    // 0 samples: at once.
    ramp.to(0.3, 0);
    assert_eq!((ramp.value(), ramp.next()), (0.3, 0.3));
    // A long one, past where adding a step a sample would drift or stall
    // in `f32`: monotone, and on its target at its last sample.
    let len = 1 << 25;
    let mut ramp = Ramp::at(1.0);
    ramp.to(0.0, len);
    let mut last = 1.0;
    for k in 1..=len {
        let value = ramp.next();
        assert!(value <= last, "{k}: {value} after {last}");
        if k == len / 2 {
            assert!((value - 0.5).abs() < 1e-6, "{value}");
        }
        last = value;
    }
    assert_eq!(last, 0.0);
}

#[test]
fn the_limiter_leaves_the_signal_below_its_knee_and_bends_it_smoothly_above() {
    for x in [0.0, 0.25, -0.5, KNEE, -KNEE] {
        assert_eq!(limit(x), x);
    }
    // Continuous at the knee, with the line's slope.
    let above = limit(KNEE + 1e-4);
    assert!((above - (KNEE + 1e-4)).abs() < 1e-6, "{above}");
    // Odd, rising, and below full scale however loud.
    let mut last = 0.0;
    for k in 1..=20_000 {
        let x = k as f32 * 1e-3;
        let y = limit(x);
        assert!(y >= last && y <= CEILING, "{x}: {y}");
        assert_eq!(limit(-x), -y);
        last = y;
    }
    // A soft bend, not a clamp: full scale comes out halfway from the knee.
    assert!((limit(1.0) - (KNEE + (1.0 - KNEE) * 0.5)).abs() < 1e-6);
    assert!(limit(0.95) < 0.95 && limit(0.95) > KNEE);
    for x in [1e6, 3e5, 1e30, f32::MAX, f32::INFINITY] {
        assert!(limit(x) <= CEILING && limit(x) > 0.999, "{x}: {}", limit(x));
        assert_eq!(limit(-x), -limit(x));
    }
    const { assert!(CEILING < 1.0) };
    assert_eq!(limit(f32::NAN), 0.0);
}

#[test]
fn the_pan_law_maps_its_ends_and_centre() {
    let [l, r] = pan_gains(0.0, 1);
    assert_eq!(l, r);
    assert!((l - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-7);
    assert_eq!(pan_gains(-1.0, 1)[0], 1.0);
    assert!(pan_gains(-1.0, 1)[1].abs() < 1e-7);
    assert!(pan_gains(1.0, 1)[0].abs() < 1e-7);
    assert_eq!(pan_gains(0.0, 2), [1.0, 1.0]);
    assert_eq!(pan_gains(-0.5, 2)[0], 1.0);
    assert_eq!(pan_gains(0.5, 2)[1], 1.0);
}

#[test]
fn attenuation_is_full_near_silent_far_and_linear_between() {
    assert_eq!(attenuation(0.0, 64.0, 800.0), 1.0);
    assert_eq!(attenuation(64.0, 64.0, 800.0), 1.0);
    assert_eq!(attenuation(432.0, 64.0, 800.0), 0.5);
    assert_eq!(attenuation(800.0, 64.0, 800.0), 0.0);
    assert_eq!(attenuation(f32::INFINITY, 64.0, 800.0), 0.0);
}

#[test]
fn the_step_is_exact_at_unity_and_bounded() {
    assert_eq!(step(48_000, 48_000, 1.0), ONE);
    assert_eq!(step(24_000, 48_000, 1.0), ONE / 2);
    assert_eq!(step(384_000, 8_000, 16.0), MAX_STEP);
    assert_eq!(step(1, 384_000, 1.0 / 16.0), 699);
}

/// Runs `render` on another thread and waits five seconds for it: a
/// render that waits for a lock fails the test instead of hanging it.
fn render_elsewhere(mixer: crate::Mixer) -> crate::Mixer {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut mixer = mixer;
        let mut out = vec![0.0; 512];
        mixer.render(&mut out, 2, 48_000);
        send.send(mixer).ok();
    });
    receive
        .recv_timeout(Duration::from_secs(5))
        .expect("render waited for a lock")
}

#[test]
fn render_never_waits_for_the_main_thread() {
    let (audio, mixer) = crate::new(AudioConfig::default()).unwrap();
    // Not a handle: it counts the queues' references, which a server's would add to.
    let sound = Sound::new(SoundData::new(1, 48_000, vec![0.1; 10]).unwrap());
    audio.play_sound(&sound, PlayParams::default());
    // The main thread holds the queue: the command waits.
    let queue = lock(&audio.shared.commands);
    let mixer = render_elsewhere(mixer);
    assert_eq!(queue.len(), 1);
    drop(queue);
    let mixer = render_elsewhere(mixer);
    assert_eq!(lock(&audio.shared.commands).len(), 0);
    assert_eq!(Arc::strong_count(sound.data()), 2, "the voice ended");
    // The main thread holds the return queue, which has the sound: played
    // again, it ends again, and waits with the mixer.
    assert_eq!(lock(&audio.shared.returns).len(), 1);
    audio.play_sound(&sound, PlayParams::default());
    let returns = lock(&audio.shared.returns);
    let mixer = render_elsewhere(mixer);
    assert_eq!(returns.len(), 1);
    assert_eq!(Arc::strong_count(sound.data()), 3);
    drop(returns);
    let mixer = render_elsewhere(mixer);
    assert_eq!(lock(&audio.shared.returns).len(), 2);
    audio.update();
    assert_eq!(Arc::strong_count(sound.data()), 1);
    // A queue a panic poisoned is taken as it is, on both sides.
    let shared = Arc::clone(&audio.shared);
    let _ = thread::spawn(move || {
        let _queue = lock(&shared.commands);
        panic!("poisons the queue");
    })
    .join();
    assert!(audio.shared.commands.is_poisoned());
    audio.play_sound(&sound, PlayParams::default());
    let _mixer = render_elsewhere(mixer);
    assert_eq!(lock(&audio.shared.commands).len(), 0);
}

#[test]
fn the_queues_never_grow() {
    let config = AudioConfig {
        voices: 2,
        commands: 16,
        ..AudioConfig::default()
    };
    let (audio, mut mixer) = crate::new(config).unwrap();
    let capacity = |mutex: &Mutex<std::collections::VecDeque<_>>| lock(mutex).capacity();
    let queue = capacity(&audio.shared.commands);
    let returns = lock(&audio.shared.returns).capacity();
    // Not a handle: the queues are the point, and a made sound fills them as a loaded one.
    let sound = Sound::new(SoundData::new(1, 48_000, vec![0.1; 4]).unwrap());
    let mut out = vec![0.0; 512];
    for round in 0..50 {
        for _ in 0..40 {
            audio.play_sound(&sound, PlayParams::default());
        }
        mixer.render(&mut out, 2, 48_000);
        if round % 7 == 0 {
            audio.update();
        }
        assert_eq!(capacity(&audio.shared.commands), queue);
        assert_eq!(lock(&audio.shared.returns).capacity(), returns);
        assert_eq!(mixer.retired_capacity(), returns);
    }
}
