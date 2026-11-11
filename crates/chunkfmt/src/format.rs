// SPDX-License-Identifier: MPL-2.0
//! Self-describing chunk object format (DESIGN.md §3).
//!
//! ```text
//! offset  size  field
//! 0       4     magic "CCH1"
//! 4       1     format version (1)
//! 5       2     codec id (LE; registry in `codec`)
//! 7       1     codec level (i8)
//! 8       8     uncompressed length (LE)
//! 16      ...   payload
//! ```

use crate::codec::{self, Codec, CompressionSetting};
use crate::error::Error;
use std::io::Write;

const MAGIC: &[u8; 4] = b"CCH1";
const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 16;

struct HashWriter<W> {
    inner: W,
    hasher: blake3::Hasher,
    bytes: u64,
    /// The header's declared length: no byte past it reaches `inner`.
    limit: u64,
    overflowed: bool,
}

impl<W: Write> Write for HashWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Enforced here, per write, rather than by the caller after each
        // `write_payload`: a bomb's compressed bytes arrive in one network
        // read, and the zstd decoder expands all of them inside that one call.
        if self.bytes.saturating_add(buf.len() as u64) > self.limit {
            self.overflowed = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "decoded output exceeds the header length",
            ));
        }
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        self.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

enum DecoderInner<W: Write> {
    Raw(HashWriter<W>),
    Zstd(zstd::stream::write::Decoder<'static, HashWriter<W>>),
    /// LZ4 blocks cannot be decoded incrementally: the (bounded) payload is
    /// buffered and decoded in `finish`.
    #[cfg(feature = "lz4")]
    Lz4(HashWriter<W>, Vec<u8>),
}

/// Incrementally decodes one stored chunk object into a caller-owned writer.
pub struct StreamingDecoder<W: Write> {
    inner: DecoderInner<W>,
    expected_len: u64,
}

impl<W: Write> StreamingDecoder<W> {
    pub fn new(header: &[u8; HEADER_LEN], out: W, hasher: blake3::Hasher) -> Result<Self, Error> {
        if &header[..4] != MAGIC {
            return Err(Error::CorruptObject("bad magic".into()));
        }
        if header[4] != VERSION {
            return Err(Error::CorruptObject(format!(
                "unsupported format version {}; upgrade this software",
                header[4]
            )));
        }
        let codec = Codec::from_id(u16::from_le_bytes(header[5..7].try_into().unwrap()))?;
        let expected_len = u64::from_le_bytes(header[8..16].try_into().unwrap());
        // The declared length is attacker-controlled; refuse one past the
        // decompression ceiling before streaming any bytes, so the writer's
        // per-write bound can't be defeated by simply declaring a huge
        // length. See `codec::max_decompressed_len`.
        if expected_len > codec::max_decompressed_len() {
            return Err(codec::ceiling_error(
                "declared decoded length",
                expected_len,
            ));
        }
        let writer = HashWriter {
            inner: out,
            hasher,
            bytes: 0,
            limit: expected_len,
            overflowed: false,
        };
        let inner = match codec {
            Codec::Raw => DecoderInner::Raw(writer),
            #[cfg(feature = "lz4")]
            Codec::Lz4 => DecoderInner::Lz4(writer, Vec::new()),
            Codec::Zstd => DecoderInner::Zstd(
                zstd::stream::write::Decoder::new(writer)
                    .map_err(|error| Error::Compression(error.to_string()))?,
            ),
        };
        Ok(Self {
            inner,
            expected_len,
        })
    }

    pub fn write_payload(&mut self, bytes: &[u8]) -> Result<(), Error> {
        #[cfg(feature = "lz4")]
        if let DecoderInner::Lz4(_, buf) = &mut self.inner {
            // The worst-case LZ4 block for `expected_len` bytes.
            let cap = self
                .expected_len
                .saturating_add(self.expected_len / 255 + 16);
            if (buf.len() as u64).saturating_add(bytes.len() as u64) > cap {
                return Err(Error::CorruptObject(format!(
                    "lz4 payload exceeds the bound for header length {}",
                    self.expected_len
                )));
            }
            buf.extend_from_slice(bytes);
            return Ok(());
        }
        let result = match &mut self.inner {
            #[cfg(feature = "lz4")]
            DecoderInner::Lz4(..) => unreachable!("handled above"),
            DecoderInner::Raw(writer) => writer.write_all(bytes),
            DecoderInner::Zstd(decoder) => decoder.write_all(bytes),
        };
        // `finish` checks the exact length only after the whole payload is
        // consumed; the writer refuses output past the header as it is
        // produced, so a crafted frame (a few compressed bytes expanding to
        // gigabytes) never streams into the caller's sink (a cache file).
        result.map_err(|error| {
            let overflowed = match &self.inner {
                #[cfg(feature = "lz4")]
                DecoderInner::Lz4(writer, _) => writer.overflowed,
                DecoderInner::Raw(writer) => writer.overflowed,
                DecoderInner::Zstd(decoder) => decoder.get_ref().overflowed,
            };
            if overflowed {
                Error::CorruptObject(format!(
                    "decoded output exceeds header {}",
                    self.expected_len
                ))
            } else {
                error.into()
            }
        })
    }

    /// Finish decoding and return `(plaintext bytes, plaintext hash)`.
    pub fn finish(self) -> Result<(u64, blake3::Hash), Error> {
        let writer = match self.inner {
            #[cfg(feature = "lz4")]
            DecoderInner::Lz4(mut writer, buf) => {
                let mut out = vec![0u8; self.expected_len as usize];
                let n = lz4_flex::block::decompress_into(&buf, &mut out)
                    .map_err(|e| Error::Compression(e.to_string()))?;
                out.truncate(n);
                writer.write_all(&out)?;
                writer
            }
            DecoderInner::Raw(writer) => writer,
            DecoderInner::Zstd(mut decoder) => {
                decoder
                    .flush()
                    .map_err(|error| Error::Compression(error.to_string()))?;
                decoder.into_inner()
            }
        };
        if writer.bytes != self.expected_len {
            return Err(Error::CorruptObject(format!(
                "decoded length {} != header {}",
                writer.bytes, self.expected_len
            )));
        }
        Ok((writer.bytes, writer.hasher.finalize()))
    }
}

/// Encode plaintext chunk data into a stored object, applying the given
/// compression setting (with the incompressible guard).
pub fn encode_object(data: &[u8], setting: CompressionSetting) -> Result<Vec<u8>, Error> {
    let (codec, level, payload) = match setting.compress(data)? {
        Some(compressed) => (setting.codec, setting.level, compressed),
        None => (Codec::Raw, 0i8, data.to_vec()),
    };
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&codec.id().to_le_bytes());
    out.push(level as u8);
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode a stored object back to plaintext chunk data.
pub fn decode_object(obj: &[u8]) -> Result<Vec<u8>, Error> {
    if obj.len() < HEADER_LEN {
        return Err(Error::CorruptObject("truncated header".into()));
    }
    if &obj[..4] != MAGIC {
        return Err(Error::CorruptObject("bad magic".into()));
    }
    if obj[4] != VERSION {
        return Err(Error::CorruptObject(format!(
            "unsupported format version {}; upgrade this software",
            obj[4]
        )));
    }
    let codec = Codec::from_id(u16::from_le_bytes(obj[5..7].try_into().unwrap()))?;
    let uncompressed_len = u64::from_le_bytes(obj[8..16].try_into().unwrap());
    let payload = &obj[HEADER_LEN..];
    let data = codec::decompress(codec, payload, uncompressed_len)?;
    if codec == Codec::Raw && data.len() as u64 != uncompressed_len {
        return Err(Error::CorruptObject(format!(
            "raw payload length {} != header {}",
            data.len(),
            uncompressed_len
        )));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_zstd() {
        let data = vec![7u8; 65536];
        let obj = encode_object(&data, CompressionSetting::zstd(5).unwrap()).unwrap();
        assert!(obj.len() < data.len());
        assert_eq!(decode_object(&obj).unwrap(), data);
    }

    #[cfg(feature = "lz4")]
    #[test]
    fn lz4_roundtrip_buffered_and_streaming() {
        let data = vec![7u8; 65_537];
        let obj = encode_object(&data, CompressionSetting::LZ4).unwrap();
        assert!(obj.len() < data.len());
        assert_eq!(u16::from_le_bytes(obj[5..7].try_into().unwrap()), 2);
        assert_eq!(decode_object(&obj).unwrap(), data);
        let header: [u8; HEADER_LEN] = obj[..HEADER_LEN].try_into().unwrap();
        let mut out = Vec::new();
        let mut d = StreamingDecoder::new(&header, &mut out, blake3::Hasher::new()).unwrap();
        for piece in obj[HEADER_LEN..].chunks(101) {
            d.write_payload(piece).unwrap();
        }
        let (n, h) = d.finish().unwrap();
        assert_eq!((n, h), (data.len() as u64, blake3::hash(&data)));
        assert_eq!(out, data);
    }

    #[test]
    fn roundtrip_raw() {
        let data = b"hello world".to_vec();
        let obj = encode_object(&data, CompressionSetting::RAW).unwrap();
        assert_eq!(obj.len(), HEADER_LEN + data.len());
        assert_eq!(decode_object(&obj).unwrap(), data);
    }

    #[test]
    fn incompressible_stored_raw() {
        let data: Vec<u8> = (0..4096u32)
            .flat_map(|i| i.wrapping_mul(2654435761).to_le_bytes())
            .collect();
        let obj = encode_object(&data, CompressionSetting::zstd(19).unwrap()).unwrap();
        // Header says raw.
        assert_eq!(
            u16::from_le_bytes(obj[5..7].try_into().unwrap()),
            crate::codec::CODEC_RAW
        );
        assert_eq!(decode_object(&obj).unwrap(), data);
    }

    #[test]
    fn corrupt_rejected() {
        assert!(decode_object(b"short").is_err());
        let data = b"payload".to_vec();
        let mut obj = encode_object(&data, CompressionSetting::RAW).unwrap();
        obj[0] = b'X';
        assert!(decode_object(&obj).is_err());
        let mut obj2 = encode_object(&data, CompressionSetting::RAW).unwrap();
        obj2[4] = 99; // future version
        assert!(decode_object(&obj2)
            .unwrap_err()
            .to_string()
            .contains("version"));
        let mut obj3 = encode_object(&data, CompressionSetting::RAW).unwrap();
        obj3[5] = 0xff; // unknown codec
        obj3[6] = 0xff;
        assert!(matches!(decode_object(&obj3), Err(Error::UnknownCodec(_))));
    }

    #[test]
    fn streaming_decode_matches_buffered_for_raw_and_zstd() {
        for setting in [
            CompressionSetting::RAW,
            CompressionSetting::zstd(3).unwrap(),
        ] {
            let data = vec![7u8; 65_537];
            let encoded = encode_object(&data, setting).unwrap();
            let header: [u8; HEADER_LEN] = encoded[..HEADER_LEN].try_into().unwrap();
            let mut out = Vec::new();
            let mut decoder =
                StreamingDecoder::new(&header, &mut out, blake3::Hasher::new()).unwrap();
            for piece in encoded[HEADER_LEN..].chunks(257) {
                decoder.write_payload(piece).unwrap();
            }
            let (bytes, hash) = decoder.finish().unwrap();
            assert_eq!(bytes, data.len() as u64);
            assert_eq!(hash, blake3::hash(&data));
            assert_eq!(out, data);
        }
    }

    fn zstd_object(declared_len: u64, payload: &[u8]) -> Vec<u8> {
        let mut obj = Vec::new();
        obj.extend_from_slice(MAGIC);
        obj.push(VERSION);
        obj.extend_from_slice(&crate::codec::CODEC_ZSTD.to_le_bytes());
        obj.push(19i8 as u8);
        obj.extend_from_slice(&declared_len.to_le_bytes());
        obj.extend_from_slice(payload);
        obj
    }

    /// A zstd payload that expands far past what its header declares is
    /// rejected rather than decompressed in full, on both decode paths; a
    /// header declaring more than the ceiling is refused before any
    /// decompression at all.
    #[test]
    fn a_decompression_bomb_is_rejected() {
        // 8 MiB of zeros compresses to a minuscule fraction of itself.
        let bomb = zstd::encode_all(&vec![0u8; 8 << 20][..], 19).unwrap();
        assert!(bomb.len() < (8 << 20) / 1000, "{} bytes", bomb.len());

        // Lying header: claims 64 bytes, really expands to 8 MiB.
        let obj = zstd_object(64, &bomb);
        assert!(matches!(
            decode_object(&obj).unwrap_err(),
            Error::CorruptObject(_)
        ));
        let header: [u8; HEADER_LEN] = obj[..HEADER_LEN].try_into().unwrap();
        let mut out = Vec::new();
        let mut decoder = StreamingDecoder::new(&header, &mut out, blake3::Hasher::new()).unwrap();
        // The whole bomb arrives in one call, as one network read would.
        assert!(matches!(
            decoder.write_payload(&obj[HEADER_LEN..]),
            Err(Error::CorruptObject(_))
        ));
        drop(decoder);
        assert!(out.len() <= 64, "{} bytes reached the sink", out.len());

        // Over-ceiling header: refused up front, naming the knob.
        let obj = zstd_object(crate::codec::max_decompressed_len() + 1, &bomb);
        match decode_object(&obj) {
            Err(Error::CorruptObject(msg)) => {
                assert!(msg.contains(crate::codec::MAX_DECOMPRESSED_ENV), "{msg}")
            }
            other => panic!("expected a ceiling error, got {other:?}"),
        }
        let header: [u8; HEADER_LEN] = obj[..HEADER_LEN].try_into().unwrap();
        assert!(StreamingDecoder::new(&header, &mut out, blake3::Hasher::new()).is_err());
    }

    #[test]
    fn streaming_decode_rejects_truncation() {
        let data = vec![9u8; 65_537];
        let encoded = encode_object(&data, CompressionSetting::zstd(3).unwrap()).unwrap();
        let header: [u8; HEADER_LEN] = encoded[..HEADER_LEN].try_into().unwrap();
        let mut out = Vec::new();
        let mut decoder = StreamingDecoder::new(&header, &mut out, blake3::Hasher::new()).unwrap();
        decoder
            .write_payload(&encoded[HEADER_LEN..encoded.len() - 2])
            .unwrap();
        assert!(decoder.finish().is_err());
    }
}
