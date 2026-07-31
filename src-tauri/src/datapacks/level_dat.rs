//! A world's `level.dat`: gzip-or-raw NBT, edited as an untyped
//! `fastnbt::Value` so every tag we do not model — `Player`, `GameRules`,
//! `WorldGenSettings`, and ~40 others — round-trips untouched. A typed struct
//! would destroy them on save. Same discipline as `crate::servers::nbt`, which
//! this is modelled on; the difference is the compression layer.
//!
//! Two rules that are easy to get wrong:
//!   * Never compare `level.dat` byte-for-byte. `Value::Compound` is a
//!     `HashMap`, so key order is lost on rewrite. Compare parsed `Value`s.
//!   * That every real `level.dat` is gzip-framed is a Minecraft-format
//!     assumption, not something this repo can prove. Sniff the magic and
//!     re-emit in whatever framing was read.

use std::io::{Read, Write};

use fastnbt::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Gzip,
    Raw,
}

fn parse_err(e: impl std::fmt::Display) -> Error {
    Error::LevelDatParse {
        reason: e.to_string(),
    }
}

/// Parse `bytes`, detecting the framing from the gzip magic `1f 8b`.
pub fn parse(bytes: &[u8]) -> Result<(Value, Framing)> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut buf = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut buf)
            .map_err(|e| parse_err(format!("gzip: {e}")))?;
        let v = fastnbt::from_bytes(&buf).map_err(parse_err)?;
        Ok((v, Framing::Gzip))
    } else {
        let v = fastnbt::from_bytes(bytes).map_err(parse_err)?;
        Ok((v, Framing::Raw))
    }
}

/// Serialize in the given framing.
pub fn serialize(root: &Value, framing: Framing) -> Result<Vec<u8>> {
    let plain = fastnbt::to_bytes(root).map_err(parse_err)?;
    match framing {
        Framing::Raw => Ok(plain),
        Framing::Gzip => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&plain)
                .map_err(|e| parse_err(format!("gzip: {e}")))?;
            enc.finish().map_err(|e| parse_err(format!("gzip: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;
    use std::collections::HashMap;

    fn sample_root() -> Value {
        let mut data = HashMap::new();
        data.insert("LevelName".to_string(), Value::String("Survival".into()));
        data.insert("SpawnX".to_string(), Value::Int(42));
        let mut root = HashMap::new();
        root.insert("Data".to_string(), Value::Compound(data));
        Value::Compound(root)
    }

    #[test]
    fn gzip_round_trip_reparses_to_the_same_value() {
        let bytes = serialize(&sample_root(), Framing::Gzip).unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b], "must be gzip framed");
        let (back, framing) = parse(&bytes).unwrap();
        assert_eq!(framing, Framing::Gzip);
        // Compare parsed Values, never bytes: fastnbt's Compound is a HashMap,
        // so key order is not preserved across a rewrite.
        assert_eq!(back, sample_root());
    }

    #[test]
    fn raw_round_trip_reparses_to_the_same_value() {
        let bytes = serialize(&sample_root(), Framing::Raw).unwrap();
        assert_ne!(&bytes[..2], &[0x1f, 0x8b]);
        let (back, framing) = parse(&bytes).unwrap();
        assert_eq!(framing, Framing::Raw);
        assert_eq!(back, sample_root());
    }

    #[test]
    fn parse_detects_framing_from_the_magic_not_from_a_flag() {
        let raw = serialize(&sample_root(), Framing::Raw).unwrap();
        let gz = serialize(&sample_root(), Framing::Gzip).unwrap();
        assert_eq!(parse(&raw).unwrap().1, Framing::Raw);
        assert_eq!(parse(&gz).unwrap().1, Framing::Gzip);
    }

    #[test]
    fn garbage_yields_level_dat_parse_not_a_panic() {
        let err = parse(b"absolutely not nbt").unwrap_err();
        assert!(matches!(err, crate::error::Error::LevelDatParse { .. }));
    }
}
