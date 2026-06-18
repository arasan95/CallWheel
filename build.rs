use std::{env, fs, path::PathBuf};

const ICON_SIZES: [usize; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() {
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
    let images: Vec<Vec<u8>> = ICON_SIZES
        .iter()
        .map(|&size| make_icon_image(size))
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

fn make_icon_image(size: usize) -> Vec<u8> {
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

    for y in (0..height).rev() {
        for x in 0..width {
            let (r, g, b, a) = icon_pixel(x as f32, y as f32, size as f32);
            data.extend_from_slice(&[b, g, r, a]);
        }
    }

    data.resize(image_bytes, 0);
    data
}

fn icon_pixel(x: f32, y: f32, size: f32) -> (u8, u8, u8, u8) {
    let center = (size - 1.0) * 0.5;
    let dx = x - center;
    let dy = y - center;
    let dist = (dx * dx + dy * dy).sqrt();

    let outer = size * 0.49;
    let ring_outer = size * 0.43;
    let ring_inner = size * 0.31;
    let core = size * 0.20;

    let alpha = smooth_fill(outer - dist, size * 0.018);
    if alpha <= 0.001 {
        return (0, 0, 0, 0);
    }

    let mut r = 8.0;
    let mut g = 16.0;
    let mut b = 28.0;

    let ring_t = smooth_step(ring_outer, ring_inner, dist);
    r = mix(12.0, r, ring_t);
    g = mix(148.0, g, ring_t);
    b = mix(208.0, b, ring_t);

    let edge = smooth_band(
        dist,
        outer - size * 0.032,
        outer - size * 0.005,
        size * 0.01,
    );
    r = mix(r, 86.0, edge);
    g = mix(g, 236.0, edge);
    b = mix(b, 245.0, edge);

    let core_t = smooth_fill(core - dist, size * 0.015);
    r = mix(r, 5.0, core_t);
    g = mix(g, 10.0, core_t);
    b = mix(b, 20.0, core_t);

    let line_width = size * 0.07;
    let slash = (dx + dy * 0.85).abs();
    let slash_t = smooth_fill(line_width - slash, size * 0.015);
    r = mix(r, 250.0, slash_t);
    g = mix(g, 250.0, slash_t);
    b = mix(b, 255.0, slash_t);

    let tip_x = center + size * 0.20;
    let tip_y = center - size * 0.22;
    let tdx = x - tip_x;
    let tdy = y - tip_y;
    let tip_d = (tdx * tdx + tdy * tdy).sqrt();
    let tip_t = smooth_fill(size * 0.09 - tip_d, size * 0.02);
    r = mix(r, 120.0, tip_t);
    g = mix(g, 245.0, tip_t);
    b = mix(b, 255.0, tip_t);

    (
        r.round() as u8,
        g.round() as u8,
        b.round() as u8,
        (alpha * 255.0).round() as u8,
    )
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn smooth_fill(value: f32, feather: f32) -> f32 {
    if feather <= 0.0 {
        return if value >= 0.0 { 1.0 } else { 0.0 };
    }
    ((value / feather) * 0.5 + 0.5).clamp(0.0, 1.0)
}

fn smooth_step(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smooth_band(x: f32, min: f32, max: f32, feather: f32) -> f32 {
    let enter = smooth_fill(x - min, feather);
    let leave = 1.0 - smooth_fill(x - max, feather);
    (enter * leave).clamp(0.0, 1.0)
}
