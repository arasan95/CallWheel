use std::{env, fs, path::PathBuf};

use image::{RgbaImage, imageops::FilterType};

const ICON_SIZES: [usize; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() {
    println!("cargo:rerun-if-changed=assets/app_icon.png");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let icon_path = out_dir.join("callwheel.ico");
    fs::write(&icon_path, make_icon()).expect("write generated application icon");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(icon_path.to_string_lossy().as_ref());
    resource
        .compile()
        .expect("embed generated application icon");
}

fn make_icon() -> Vec<u8> {
    let source = load_source_icon();
    let images: Vec<Vec<u8>> = ICON_SIZES
        .iter()
        .map(|&size| make_icon_image(&source, size))
        .collect();
    let entry_count = images.len();
    let dir_size = 6 + entry_count * 16;
    let total_size = dir_size + images.iter().map(Vec::len).sum::<usize>();
    let mut ico = Vec::with_capacity(total_size);

    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&(entry_count as u16).to_le_bytes());

    let mut offset = dir_size as u32;
    for (size, image) in ICON_SIZES.iter().zip(images.iter()) {
        ico.push(if *size == 256 { 0 } else { *size as u8 });
        ico.push(if *size == 256 { 0 } else { *size as u8 });
        ico.push(0);
        ico.push(0);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }

    for image in images {
        ico.extend_from_slice(&image);
    }
    ico
}

fn load_source_icon() -> RgbaImage {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    image::open(manifest_dir.join("assets").join("app_icon.png"))
        .expect("load assets/app_icon.png")
        .into_rgba8()
}

fn make_icon_image(source: &RgbaImage, size: usize) -> Vec<u8> {
    let width = size;
    let height = size;
    let xor_bytes = width * height * 4;
    let mask_stride = width.div_ceil(32) * 4;
    let mask_bytes = mask_stride * height;
    let image_bytes = 40 + xor_bytes + mask_bytes;
    let mut data = Vec::with_capacity(image_bytes);

    data.extend_from_slice(&(40u32).to_le_bytes());
    data.extend_from_slice(&(width as i32).to_le_bytes());
    data.extend_from_slice(&((height * 2) as i32).to_le_bytes());
    data.extend_from_slice(&(1u16).to_le_bytes());
    data.extend_from_slice(&(32u16).to_le_bytes());
    data.extend_from_slice(&(0u32).to_le_bytes());
    data.extend_from_slice(&(xor_bytes as u32).to_le_bytes());
    data.extend_from_slice(&(0i32).to_le_bytes());
    data.extend_from_slice(&(0i32).to_le_bytes());
    data.extend_from_slice(&(0u32).to_le_bytes());
    data.extend_from_slice(&(0u32).to_le_bytes());

    let resized = image::imageops::resize(source, size as u32, size as u32, FilterType::Lanczos3);
    for y in (0..height).rev() {
        for x in 0..width {
            let pixel = resized.get_pixel(x as u32, y as u32).0;
            let [r, g, b, a] = pixel;
            data.extend_from_slice(&[b, g, r, a]);
        }
    }

    data.resize(image_bytes, 0);
    data
}
