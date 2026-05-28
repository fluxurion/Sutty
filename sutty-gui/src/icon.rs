//! Parse a minimal subset of the ICO format to extract the largest icon as RGBA.
//! ICO files contain a 6-byte header followed by one or more 16-byte directory
//! entries pointing to DIB (BMP-without-file-header) image data.

pub fn load_icon(ico_bytes: &[u8]) -> Option<egui::IconData> {
    if ico_bytes.len() < 6 {
        return None;
    }
    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]) as usize;
    if count == 0 {
        return None;
    }

    // Scan directory entries to find the largest (by area) icon
    let mut best: Option<(u32, u32, &[u8])> = None; // (width, height, dib_data)

    for i in 0..count {
        let off = 6 + i * 16;
        if off + 16 > ico_bytes.len() {
            break;
        }
        let entry = &ico_bytes[off..off + 16];
        let w = if entry[0] == 0 { 256 } else { entry[0] as u32 };
        let h = if entry[1] == 0 { 256 } else { entry[1] as u32 };
        let bpp = u16::from_le_bytes([entry[6], entry[7]]);
        let size = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let data_offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;

        // Only consider 32 bpp icons for simplicity
        if bpp != 32 {
            continue;
        }
        if data_offset + size > ico_bytes.len() {
            continue;
        }

        let area = w * h;
        if best.as_ref().map_or(true, |b| area > b.0 * b.1) {
            best = Some((w, h, &ico_bytes[data_offset..data_offset + size]));
        }
    }

    let (width, height, dib) = best?;

    // DIB header tells us where pixel data starts.
    // For 32 bpp, the header is typically a BITMAPINFOHEADER (40 bytes).
    if dib.len() < 40 {
        return None;
    }
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    if header_size < 40 || dib.len() < header_size {
        return None;
    }

    let pixels = &dib[header_size..];
    let expected = (width * height * 4) as usize;
    if pixels.len() < expected {
        return None;
    }
    let pixels = &pixels[..expected];

    // Convert BGRA → RGBA
    let mut rgba = Vec::with_capacity(expected);
    for chunk in pixels.chunks_exact(4) {
        rgba.push(chunk[2]); // R
        rgba.push(chunk[1]); // G
        rgba.push(chunk[0]); // B
        rgba.push(chunk[3]); // A
    }

    Some(egui::IconData {
        rgba,
        width,
        height,
    })
}
