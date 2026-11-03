//! BPS patch application (the format used by ROM hacks and game mods).
//! Spec: https://github.com/blakesmith/rombp/blob/master/docs/bps_spec.md

const MAGIC: &[u8] = b"BPS1";
const FOOTER_LEN: usize = 12;

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.data.get(self.pos).ok_or("truncated patch")?;
        self.pos += 1;
        Ok(b)
    }

    fn number(&mut self) -> Result<u64, String> {
        let (mut value, mut shift) = (0u64, 1u64);
        loop {
            let b = self.byte()?;
            value = value
                .checked_add((b & 0x7F) as u64 * shift)
                .ok_or("corrupt patch: number overflow")?;
            if b & 0x80 != 0 {
                return Ok(value);
            }
            shift = shift
                .checked_shl(7)
                .ok_or("corrupt patch: number overflow")?;
            value += shift;
        }
    }

    /// A signed relative offset (sign in the lowest bit).
    fn offset(&mut self) -> Result<i64, String> {
        let n = self.number()?;
        let magnitude = (n >> 1) as i64;
        Ok(if n & 1 != 0 { -magnitude } else { magnitude })
    }

    fn take(&mut self, len: usize) -> Result<&[u8], String> {
        let slice = self
            .data
            .get(self.pos..self.pos + len)
            .ok_or("truncated patch")?;
        self.pos += len;
        Ok(slice)
    }
}

fn footer_crc(patch: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(patch[at..at + 4].try_into().unwrap())
}

fn moved(base: usize, delta: i64) -> Result<usize, String> {
    usize::try_from(base as i64 + delta)
        .map_err(|_| "corrupt patch: offset out of range".to_string())
}

/// Apply `patch` to `source`, verifying the source, target and patch checksums.
pub fn apply(patch: &[u8], source: &[u8]) -> Result<Vec<u8>, String> {
    if patch.len() < MAGIC.len() + FOOTER_LEN || !patch.starts_with(MAGIC) {
        return Err("not a BPS patch".into());
    }
    let body_end = patch.len() - FOOTER_LEN;
    if crc32fast::hash(&patch[..patch.len() - 4]) != footer_crc(patch, patch.len() - 4) {
        return Err("patch file is corrupt (checksum mismatch)".into());
    }
    if crc32fast::hash(source) != footer_crc(patch, body_end) {
        return Err("source file does not match the patch".into());
    }

    let mut cur = Cursor {
        data: &patch[..body_end],
        pos: MAGIC.len(),
    };
    let source_len = cur.number()? as usize;
    let target_len = cur.number()? as usize;
    let metadata_len = cur.number()? as usize;
    cur.take(metadata_len)?;
    if source_len != source.len() {
        return Err("source file size does not match the patch".into());
    }

    let mut out: Vec<u8> = Vec::with_capacity(target_len);
    let (mut source_rel, mut target_rel) = (0usize, 0usize);
    while cur.pos < body_end {
        let word = cur.number()?;
        let len = (word >> 2) as usize + 1;
        match word & 3 {
            0 => {
                let at = out.len();
                out.extend_from_slice(
                    source
                        .get(at..at + len)
                        .ok_or("corrupt patch: source read out of range")?,
                );
            }
            1 => out.extend_from_slice(cur.take(len)?),
            2 => {
                source_rel = moved(source_rel, cur.offset()?)?;
                out.extend_from_slice(
                    source
                        .get(source_rel..source_rel + len)
                        .ok_or("corrupt patch: source copy out of range")?,
                );
                source_rel += len;
            }
            _ => {
                target_rel = moved(target_rel, cur.offset()?)?;
                // Byte by byte: the copied range may overlap what it produces.
                for _ in 0..len {
                    let b = *out
                        .get(target_rel)
                        .ok_or("corrupt patch: target copy out of range")?;
                    out.push(b);
                    target_rel += 1;
                }
            }
        }
    }

    if out.len() != target_len || crc32fast::hash(&out) != footer_crc(patch, body_end + 4) {
        return Err("patched output does not match the expected result".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures produced with python-bps (diff_bytearrays + write_bps).
    const PATCH: &[u8] = include_bytes!("../../testdata/patch.bps");
    const SOURCE: &[u8] = include_bytes!("../../testdata/src.bin");
    const TARGET: &[u8] = include_bytes!("../../testdata/tgt.bin");

    #[test]
    fn applies_python_bps_patch() {
        assert_eq!(apply(PATCH, SOURCE).unwrap(), TARGET);
    }

    #[test]
    fn rejects_wrong_source() {
        assert!(apply(PATCH, TARGET).unwrap_err().contains("source"));
    }

    #[test]
    fn rejects_corrupt_patch() {
        let mut corrupt = PATCH.to_vec();
        corrupt[10] ^= 0xFF;
        assert!(apply(&corrupt, SOURCE).is_err());
        assert!(apply(b"nope", SOURCE).is_err());
    }
}
