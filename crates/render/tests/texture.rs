//! The texture asset without a GPU.

use std::fs;
use std::path::Path;

use nadir_assets::{AssetError, AssetServer, FinalizeCtx, LoadState, OnFailure};
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_render::Texture;

#[test]
fn a_texture_needs_the_device_in_the_context() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("nadir-render-texture");
    fs::create_dir_all(&root).expect("create the test directory");
    let header = Header {
        width: 1,
        height: 1,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    let bytes = qoi::encode(&header, &[1, 2, 3, 4]).expect("an image");
    fs::write(root.join("dot.qoi"), bytes).expect("write the file");

    let mut server = AssetServer::new(root, OnFailure::Placeholder);
    let dot = server.load::<Texture>("dot.qoi");
    server.finish_completed_loads(&mut FinalizeCtx::new());
    assert_eq!(server.state(dot), Some(LoadState::Failed));
    assert_eq!(
        server.error(dot),
        Some(&AssetError::MissingContext("nadir_gfx::device::Device"))
    );
    // The placeholder needs the device too, so there is none.
    assert_eq!(server.get(dot), None);
}
