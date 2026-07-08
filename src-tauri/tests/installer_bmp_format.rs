//! Guard the two installer bitmaps: NSIS requires BMP v3 (40-byte
//! BITMAPINFOHEADER), 24-bit, no alpha, at the exact sidebar/header dims.
//! A wrong format or size ships a broken or ugly wizard.

use std::fs;
use std::path::Path;

fn read_bmp_header(rel: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let bytes = fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    assert!(
        bytes.len() >= 54,
        "{rel}: shorter than a 54-byte BMP header"
    );
    bytes[..54].to_vec()
}

fn le_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
fn le_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn assert_bmp(rel: &str, w: u32, h: u32) {
    let hdr = read_bmp_header(rel);
    assert_eq!(&hdr[0..2], b"BM", "{rel}: not a BMP (missing BM magic)");
    assert_eq!(
        le_u32(&hdr, 14),
        40,
        "{rel}: not BMP v3 (BITMAPINFOHEADER != 40)"
    );
    assert_eq!(le_u32(&hdr, 18), w, "{rel}: width");
    assert_eq!(le_u32(&hdr, 22), h, "{rel}: height");
    assert_eq!(le_u16(&hdr, 28), 24, "{rel}: not 24-bit");
}

#[test]
fn installer_bitmaps_are_bmp3_24bit_correct_size() {
    assert_bmp("installer/nsis-sidebar.bmp", 164, 314);
    assert_bmp("installer/nsis-header.bmp", 150, 57);
}
