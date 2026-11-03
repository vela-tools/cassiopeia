//! Character-encoding detection for Cassiopeia's sources.
//!
//! A source's encoding is settled from its bytes, a byte order mark first, and from what its source
//! says about them apart from the bytes: the `charset` a server declares and the top-level domain it
//! serves from. The collect stage records that context as an [`encoding_hint::EncodingHint`], and the
//! profile stage settles the encoding with it.

mod bomless_utf16;
mod encoding_evidence;
pub mod encoding_hint;
pub mod top_level_domain;
