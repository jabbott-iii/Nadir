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

//! Shared by the integration tests: sounds and tracks written as the cook
//! writes them, with the codec's own encoders, into files in memory, and the
//! asset server that loads them.

#![allow(dead_code)] // Each test file uses its own share of these.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use nadir_assets::{
    AssetError, AssetPath, AssetServer, FinalizeCtx, Handle, OnFailure, Scope, Source,
};
use nadir_audio::{Music, Sound};
use nadir_codec::{kpcm, pcm, qoa};

/// Files in memory, to which a test can add after the server is made.
#[derive(Clone, Debug, Default)]
pub(crate) struct Files(Arc<Mutex<HashMap<String, Vec<u8>>>>);

impl Files {
    /// Puts `bytes` at `path`, in place of what was there.
    pub(crate) fn insert(&self, path: &str, bytes: Vec<u8>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_string(), bytes);
    }
}

impl Source for Files {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path.as_str())
            .cloned()
            .ok_or(AssetError::NotFound)
    }
}

/// A server of the sounds and tracks a test makes, each written to a file
/// of its own and loaded before it returns.
pub(crate) struct Library {
    pub(crate) server: AssetServer,
    pub(crate) files: Files,
    made: usize,
}

impl Library {
    /// A server with no placeholders, as a release build has.
    pub(crate) fn new() -> Self {
        Self::with(OnFailure::Nothing)
    }

    pub(crate) fn with(on_failure: OnFailure) -> Self {
        let files = Files::default();
        Self {
            server: AssetServer::with_source(files.clone(), on_failure),
            files,
            made: 0,
        }
    }

    /// The next file's path below `folder`, with `extension`.
    fn path(&mut self, folder: &str, extension: &str) -> String {
        self.made += 1;
        format!("audio/{folder}/{}.{extension}", self.made)
    }

    /// A sound of `samples`, 16-bit and interleaved, of `channels` at
    /// `rate`, as a sound file, loaded in `scope`. At 8,000 to 384,000 Hz
    /// but for 48,000, it is converted as it loads.
    pub(crate) fn sound_in(
        &mut self,
        scope: Scope,
        channels: u8,
        rate: u32,
        samples: &[i16],
    ) -> Handle<Sound> {
        let path = self.path("sfx", "kpcm");
        let header = kpcm::Header {
            channels,
            sample_rate: rate,
            frames: u32::try_from(samples.len() / usize::from(channels)).expect("frames"),
        };
        self.files
            .insert(&path, kpcm::encode(&header, samples).expect("a sound file"));
        self.server
            .load_now_in(&path, scope, &mut FinalizeCtx::new())
            .unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    /// A sound of `samples` loaded for good.
    pub(crate) fn sound(&mut self, channels: u8, rate: u32, samples: &[i16]) -> Handle<Sound> {
        self.sound_in(Scope::Global, channels, rate, samples)
    }

    /// A sound of `f32` samples, each made 16 bits as the cook makes them:
    /// it plays [`loaded`] of each.
    pub(crate) fn sound_f32(&mut self, channels: u8, rate: u32, samples: &[f32]) -> Handle<Sound> {
        let samples: Vec<i16> = samples.iter().map(|&x| pcm::to_i16(x)).collect();
        self.sound(channels, rate, &samples)
    }

    /// A mono sound of `frames` frames at `level`, at 48 kHz.
    pub(crate) fn constant(&mut self, level: f32, frames: usize) -> Handle<Sound> {
        self.sound_f32(1, 48_000, &vec![level; frames])
    }

    /// A track of `samples`, 16-bit and interleaved, of `channels` at
    /// `rate`, as a QOA file, loaded in `scope`.
    pub(crate) fn track_in(
        &mut self,
        scope: Scope,
        channels: u8,
        rate: u32,
        samples: &[i16],
    ) -> Handle<Music> {
        let path = self.path("music", "qoa");
        let header = qoa::Header {
            channels,
            sample_rate: rate,
            samples: u32::try_from(samples.len() / usize::from(channels)).expect("samples"),
        };
        self.files
            .insert(&path, qoa::encode(&header, samples).expect("a QOA file"));
        self.server
            .load_now_in(&path, scope, &mut FinalizeCtx::new())
            .unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    /// A track of `samples` loaded for good.
    pub(crate) fn track(&mut self, channels: u8, rate: u32, samples: &[i16]) -> Handle<Music> {
        self.track_in(Scope::Global, channels, rate, samples)
    }

    /// The sound under `handle`, which must have loaded.
    pub(crate) fn get(&self, handle: Handle<Sound>) -> &Sound {
        self.server.get(handle).expect("a loaded sound")
    }

    /// The track under `handle`, which must have loaded.
    pub(crate) fn get_track(&self, handle: Handle<Music>) -> &Music {
        self.server.get(handle).expect("a loaded track")
    }

    /// Drops what `scope` holds: the server lets go of it at once.
    pub(crate) fn unload(&mut self, scope: Scope) {
        self.server.unload_scope(scope);
        self.server.finish_all_loads(&mut FinalizeCtx::new());
    }
}

/// `level` as a file's 16 bits hold it, back as `f32`: what a loaded sound
/// plays for it.
pub(crate) fn loaded(level: f32) -> f32 {
    f32::from(pcm::to_i16(level)) / 32768.0
}
