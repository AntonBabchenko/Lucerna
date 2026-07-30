//! Minecraft protocol framing primitives for Server List Ping: VarInt and
//! length-prefixed UTF-8 strings. Pure byte pushing — no sockets, no I/O — so
//! every hostile-input case is a plain unit test.

/// Why a decode failed. Callers collapse all of these into "no answer"; the
/// distinction exists for framing decisions and tests.
#[derive(Debug, PartialEq, Eq)]
pub enum CodecError {
    /// Fewer bytes than the value needs. The stream reader treats this as
    /// "read one more byte and retry"; a slice reader treats it as malformed.
    Incomplete,
    /// A VarInt ran past 5 bytes — malformed or hostile.
    VarIntTooLong,
    /// A length prefix exceeded the caller's cap.
    TooLong,
    /// The payload was not valid UTF-8.
    NotUtf8,
}

/// Append `value` as a Minecraft VarInt (LEB128 over the u32 bit pattern, so
/// negative values occupy the full five bytes).
pub fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Decode a VarInt from the front of `buf`, returning the value and how many
/// bytes it consumed. At most 5 bytes are ever inspected, so a stream of
/// continuation bytes cannot make this read without bound.
pub fn read_varint(buf: &[u8]) -> Result<(i32, usize), CodecError> {
    let mut result: u32 = 0;
    for (i, byte) in buf.iter().take(5).enumerate() {
        result |= u32::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((result as i32, i + 1));
        }
    }
    if buf.len() >= 5 {
        // Five bytes and every one had the continuation bit set.
        return Err(CodecError::VarIntTooLong);
    }
    Err(CodecError::Incomplete)
}

/// Append `s` as a VarInt-length-prefixed UTF-8 string.
pub fn write_string(out: &mut Vec<u8>, s: &str) {
    write_varint(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

/// Decode a length-prefixed UTF-8 string, refusing a declared length above
/// `max_len` *before* touching the payload — so a hostile prefix can never make
/// us allocate or wait for megabytes.
pub fn read_string(buf: &[u8], max_len: usize) -> Result<(String, usize), CodecError> {
    let (len, used) = read_varint(buf)?;
    if len < 0 {
        return Err(CodecError::TooLong);
    }
    let len = len as usize;
    if len > max_len {
        return Err(CodecError::TooLong);
    }
    let end = used + len;
    if buf.len() < end {
        return Err(CodecError::Incomplete);
    }
    let s = std::str::from_utf8(&buf[used..end])
        .map_err(|_| CodecError::NotUtf8)?
        .to_string();
    Ok((s, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trip() {
        for v in [0i32, 1, 127, 128, 255, 2097151, i32::MAX, -1] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let (got, used) = read_varint(&buf).expect("decodes");
            assert_eq!(got, v, "value {v}");
            assert_eq!(used, buf.len(), "value {v}");
        }
    }

    #[test]
    fn varint_rejects_sixth_continuation_byte() {
        let buf = [0x80u8, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert_eq!(read_varint(&buf), Err(CodecError::VarIntTooLong));
    }

    #[test]
    fn varint_reports_incomplete_input() {
        assert_eq!(read_varint(&[0x80]), Err(CodecError::Incomplete));
        assert_eq!(read_varint(&[]), Err(CodecError::Incomplete));
    }

    #[test]
    fn string_round_trip() {
        let mut buf = Vec::new();
        write_string(&mut buf, "mc.example.net");
        let (s, used) = read_string(&buf, 64).expect("decodes");
        assert_eq!(s, "mc.example.net");
        assert_eq!(used, buf.len());
    }

    #[test]
    fn string_rejects_length_over_cap() {
        let mut buf = Vec::new();
        write_string(&mut buf, &"x".repeat(50));
        assert_eq!(read_string(&buf, 10), Err(CodecError::TooLong));
    }

    #[test]
    fn string_rejects_negative_length() {
        let mut buf = Vec::new();
        write_varint(&mut buf, -1);
        assert_eq!(read_string(&buf, 1024), Err(CodecError::TooLong));
    }

    #[test]
    fn string_reports_truncated_payload() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 10);
        buf.extend_from_slice(b"abc");
        assert_eq!(read_string(&buf, 1024), Err(CodecError::Incomplete));
    }

    #[test]
    fn string_rejects_invalid_utf8() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 2);
        buf.extend_from_slice(&[0xff, 0xfe]);
        assert_eq!(read_string(&buf, 1024), Err(CodecError::NotUtf8));
    }
}
