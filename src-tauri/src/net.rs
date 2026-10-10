//! One HTTP client for every download, so all requests identify the app the same way.

use std::sync::OnceLock;
use std::time::Duration;

use ureq::Agent;

/// Sent with every request so server owners can see who is calling.
pub const USER_AGENT: &str = concat!("Lotus/", env!("CARGO_PKG_VERSION"));

pub fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into()
    })
}

/// Percent-encodes everything except unreserved URL characters.
pub fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodes `%XX` sequences. Invalid sequences are kept as they are.
pub fn decode_path_segment(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let name = "Ash Prime (Neuroptics)+1.png";
        let encoded = encode_path_segment(name);
        assert_eq!(encoded, "Ash%20Prime%20%28Neuroptics%29%2B1.png");
        assert_eq!(decode_path_segment(&encoded), name);
        assert_eq!(decode_path_segment("bad%zz%4"), "bad%zz%4");
    }
}
