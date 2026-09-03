//! Pagination cursor codec, wire-compatible with the hosted Maestro API.
//!
//! Hosted cursors are unpadded url-safe base64 of the same byte layout this
//! server uses (VarUInt height, VarUInt intra-block index, 32-byte tx ref),
//! so cursors minted here round-trip through wallets exactly like hosted
//! ones.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

pub fn encode_cursor(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode_cursor(cursor: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(cursor).ok()
}

/// How to resume a paginated scan from a client cursor.
///
/// `Exact` resumes at the precise key, and is only used for cursors this
/// index minted whose key still exists. A structurally valid cursor whose
/// key is unknown -- a hosted-era cursor carried across the migration, a
/// cursor into an orphaned block, or a cursor into a since-rebuilt mempool
/// overlay -- resumes via `resume_fallback`, which may re-serve rows but
/// never skips any (clients de-duplicate by transaction hash).
pub enum CursorResume {
    Exact(Vec<u8>),
    HeightOnly(u64),
    /// Restart from the top of the key range: the descending overlay case,
    /// where rows may have been re-estimated above the cursor's height.
    Top,
}

/// Fallback resume position for a cursor whose exact key no longer exists.
///
/// At or below the confirmed tip, the cursor's whole block is re-served.
/// Above the tip the cursor pointed into the mempool overlay, which is
/// rebuilt on every snapshot and can move rows to either side of the
/// cursor's pseudo-height, so the entire overlay is re-served: ascending
/// restarts at tip+1, descending restarts from the top of the range.
pub fn resume_fallback(cursor_height: u64, tip_height: u64, descending: bool) -> CursorResume {
    if cursor_height <= tip_height {
        CursorResume::HeightOnly(cursor_height)
    } else if descending {
        CursorResume::Top
    } else {
        CursorResume::HeightOnly(tip_height.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_base64url() {
        let bytes = vec![0x03, 0x01, 0x3d, 0x17, 0x01, 0x96];
        assert_eq!(decode_cursor(&encode_cursor(&bytes)), Some(bytes));
    }

    #[test]
    fn decodes_a_real_hosted_cursor() {
        let hosted = "AwE9FwEBXP5C95g81KEn-nupXdW_wobjYqM2NNUfv9QYo4phvUs";
        let bytes = decode_cursor(hosted).expect("hosted cursor decodes");
        assert_eq!(bytes.len(), 38);
        assert_eq!(&bytes[..4], &[0x03, 0x01, 0x3d, 0x17]);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(decode_cursor("not/base64!or hex"), None);
    }

    #[test]
    fn fallback_below_tip_reserves_the_block() {
        assert!(matches!(
            resume_fallback(99, 100, false),
            CursorResume::HeightOnly(99)
        ));
        assert!(matches!(
            resume_fallback(100, 100, true),
            CursorResume::HeightOnly(100)
        ));
    }

    #[test]
    fn fallback_above_tip_reserves_the_overlay() {
        assert!(matches!(
            resume_fallback(105, 100, false),
            CursorResume::HeightOnly(101)
        ));
        assert!(matches!(resume_fallback(105, 100, true), CursorResume::Top));
    }
}
