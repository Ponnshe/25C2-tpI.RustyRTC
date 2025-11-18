use opencv::core::Mat;
use opencv::{core::CV_8UC3, prelude::*};

/// Always returns tightly packed RGB (len = width*height*3), regardless of stride/continuity.
pub fn tight_rgb_bytes(mat: &Mat, width: u32, height: u32) -> opencv::Result<Vec<u8>> {
    // Ensure 8UC3
    if mat.typ() != CV_8UC3 {
        let mut fixed = Mat::default();
        mat.convert_to(&mut fixed, CV_8UC3, 1.0, 0.0)?;
        return tight_rgb_bytes(&fixed, width, height);
    }

    // Force a continuous buffer if needed
    let m = if mat.is_continuous() {
        mat.try_clone()?
    } else {
        mat.clone()
    };

    let w = width as usize;
    let h = height as usize;
    let ch = m.channels() as usize; // 3
    let expected = w * h * ch;

    let data = m.data_bytes()?;

    // Fast path: already tight
    if data.len() == expected {
        return Ok(data.to_vec());
    }

    // Row-copy using actual step
    let step_elems = m.step1(0)?;
    let elem_size = m.elem_size()?;
    let step_bytes = step_elems * elem_size;

    let cols = m.cols() as usize;
    let rows = m.rows() as usize;
    let row_bytes = cols * ch;

    let mut out = vec![0u8; rows * row_bytes];
    for r in 0..rows {
        let src = &data[r * step_bytes..r * step_bytes + row_bytes];
        let dst = &mut out[r * row_bytes..(r + 1) * row_bytes];
        dst.copy_from_slice(src);
    }
    Ok(out)
}
