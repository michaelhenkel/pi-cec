//! Patch only the Pi's HDMI VSDB address, preserving all video capabilities.
use pi_cec::{sibling_routes, valid_pa};
pub fn offset(data: &[u8]) -> Result<usize, String> {
    if data.len() < 128 || data.len() % 128 != 0 {
        return Err("incomplete EDID blocks".into());
    }
    if data[..8] != [0, 255, 255, 255, 255, 255, 255, 0] {
        return Err("invalid EDID header".into());
    }
    if data.len() != (data[126] as usize + 1) * 128 {
        return Err("EDID extension count mismatch".into());
    }
    if data
        .chunks_exact(128)
        .any(|b| b.iter().fold(0u8, |s, v| s.wrapping_add(*v)) != 0)
    {
        return Err("invalid EDID checksum".into());
    }
    let mut found = Vec::new();
    for base in (128..data.len()).step_by(128) {
        if data[base] != 2 {
            continue;
        }
        let end = data[base + 2] as usize;
        if end == 0 {
            continue;
        }
        if !(4..=127).contains(&end) {
            return Err("invalid CTA boundary".into());
        }
        let mut p = base + 4;
        while p < base + end {
            let len = (data[p] & 31) as usize;
            if p + 1 + len > base + end {
                return Err("truncated CTA block".into());
            }
            if data[p] >> 5 == 3 && len >= 5 && data[p + 1..p + 4] == [3, 12, 0] {
                found.push(p + 4);
            }
            p += 1 + len;
        }
    }
    if found.len() != 1 {
        return Err(format!("expected one HDMI VSDB, found {}", found.len()));
    }
    Ok(found[0])
}
pub fn address(data: &[u8]) -> Result<u16, String> {
    let o = offset(data)?;
    let pa = u16::from_be_bytes([data[o], data[o + 1]]);
    if !valid_pa(pa) {
        return Err(format!("invalid physical address {pa:04x}"));
    }
    Ok(pa)
}
pub fn proxy(pi: &[u8], pc: &[u8]) -> Result<Vec<u8>, String> {
    let (a, b) = (address(pi)?, address(pc)?);
    if !sibling_routes(a, b) {
        return Err("Pi and PC must have distinct measured sibling paths".into());
    }
    let mut data = pi.to_vec();
    let off = offset(pi)?;
    data[off..off + 2].copy_from_slice(&b.to_be_bytes());
    let base = off / 128 * 128;
    data[base + 127] = 0u8.wrapping_sub(
        data[base..base + 127]
            .iter()
            .fold(0u8, |s, v| s.wrapping_add(*v)),
    );
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(pa: u16) -> Vec<u8> {
        let mut d = vec![0u8; 256];
        d[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
        d[126] = 1;
        d[128..132].copy_from_slice(&[2, 3, 10, 0]);
        d[132..138].copy_from_slice(&[0x65, 3, 12, 0, (pa >> 8) as u8, pa as u8]);
        for b in [0, 128] {
            d[b + 127] =
                0u8.wrapping_sub(d[b..b + 127].iter().fold(0u8, |s, v| s.wrapping_add(*v)));
        }
        d
    }
    #[test]
    fn only_pa_checksum_change() {
        let d = fixture(0x1300);
        let out = proxy(&d, &fixture(0x1200)).unwrap();
        assert_eq!(address(&out).unwrap(), 0x1200);
        assert_eq!(
            (0..256).filter(|i| d[*i] != out[*i]).collect::<Vec<_>>(),
            vec![136, 255]
        );
    }
    #[test]
    fn rejects_corruption() {
        let mut d = fixture(0x1200);
        d[50] ^= 1;
        assert!(address(&d).is_err());
    }
    #[test]
    fn rejects_unrelated() {
        for pa in [0x1300, 0x2100, 0x1310] {
            assert!(proxy(&fixture(0x1300), &fixture(pa)).is_err());
        }
    }
    #[test]
    fn rejects_truncation() {
        assert!(address(&fixture(0x1200)[..255]).is_err());
    }
    #[test]
    fn rejects_bad_pa() {
        for pa in [0, 0xffff, 0x1020] {
            assert!(address(&fixture(pa)).is_err());
        }
    }
    #[test]
    fn rejects_no_vsdb() {
        let mut d = fixture(0x1200);
        d[132] = 0x45;
        d[255] = 0u8.wrapping_sub(d[128..255].iter().fold(0u8, |s, v| s.wrapping_add(*v)));
        assert!(address(&d).is_err());
    }
    #[test]
    fn rejects_block_overrun() {
        let mut d = fixture(0x1200);
        d[132] = 0x7f;
        d[255] = 0u8.wrapping_sub(d[128..255].iter().fold(0u8, |s, v| s.wrapping_add(*v)));
        assert!(address(&d).is_err());
    }
}
