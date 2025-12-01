#[derive(Debug)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

fn clamp(v: f32) -> u8 {
    v.clamp(0.0, 255.0) as u8
}

#[allow(clippy::many_single_char_names)]
fn convert_bt601_full(y: u8, u: u8, v: u8) -> Rgb {
    let yf = y as f32;
    let uf = (u as f32) - 128.0;
    let vf = (v as f32) - 128.0;

    let r = yf + 1.402 * vf;
    let g = yf - 0.344_136 * uf - 0.714_136 * vf;
    let b = yf + 1.772 * uf;

    Rgb {
        r: clamp(r),
        g: clamp(g),
        b: clamp(b),
    }
}

#[allow(clippy::many_single_char_names)]
fn convert_bt601_limited(y: u8, u: u8, v: u8) -> Rgb {
    let y = ((y as f32 - 16.0) * (255.0 / 219.0)).max(0.0);
    let u = (u as f32 - 128.0) * (255.0 / 224.0);
    let v = (v as f32 - 128.0) * (255.0 / 224.0);

    let r = y + 1.402 * v;
    let g = y - 0.344_136 * u - 0.714_136 * v;
    let b = y + 1.772 * u;

    Rgb {
        r: clamp(r),
        g: clamp(g),
        b: clamp(b),
    }
}

#[allow(clippy::many_single_char_names)]
fn convert_bt709_full(y: u8, u: u8, v: u8) -> Rgb {
    let yf = y as f32;
    let uf = (u as f32) - 128.0;
    let vf = (v as f32) - 128.0;

    let r = yf + 1.574_80 * vf;
    let g = yf - 0.187_33 * uf - 0.468_13 * vf;
    let b = yf + 1.855_63 * uf;

    Rgb {
        r: clamp(r),
        g: clamp(g),
        b: clamp(b),
    }
}

#[allow(clippy::many_single_char_names)]
fn convert_bt709_limited(y: u8, u: u8, v: u8) -> Rgb {
    let y = ((y as f32 - 16.0) * (255.0 / 219.0)).max(0.0);
    let u = (u as f32 - 128.0) * (255.0 / 224.0);
    let v = (v as f32 - 128.0) * (255.0 / 224.0);

    let r = y + 1.574_80 * v;
    let g = y - 0.187_33 * u - 0.468_13 * v;
    let b = y + 1.855_63 * u;

    Rgb {
        r: clamp(r),
        g: clamp(g),
        b: clamp(b),
    }
}

pub fn debug_yuv_to_rgb(y: u8, u: u8, v: u8) {
    println!("=== YUV→RGB Debug ===");
    println!("Input YUV: Y={} U={} V={}", y, u, v);

    let c601_full = convert_bt601_full(y, u, v);
    let c601_lim = convert_bt601_limited(y, u, v);
    let c709_full = convert_bt709_full(y, u, v);
    let c709_lim = convert_bt709_limited(y, u, v);

    println!("\n--- Results ---");
    println!(
        "BT.601 FULL    → R:{} G:{} B:{}",
        c601_full.r, c601_full.g, c601_full.b
    );
    println!(
        "BT.601 LIMITED → R:{} G:{} B:{}",
        c601_lim.r, c601_lim.g, c601_lim.b
    );
    println!(
        "BT.709 FULL    → R:{} G:{} B:{}",
        c709_full.r, c709_full.g, c709_full.b
    );
    println!(
        "BT.709 LIMITED → R:{} G:{} B:{}",
        c709_lim.r, c709_lim.g, c709_lim.b
    );
}
