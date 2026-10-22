//! Pane tokens (ARCHITECTURE §7): 32 random bytes from the OS CSPRNG, hex-encoded, mapped to one pane,
//! compared in constant time, revoked when the pane exits. The value never appears in Debug output.

use std::fmt;

/// Secret bearer token of one pane. `Debug` is redacted and there is no `Display`, so the value cannot slip into
/// logs or error messages by accident; `expose` is for building the child's environment only.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// The hex value, for `AGENTDECK_TOKEN` only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// Equal-length inputs are compared without an early exit. Length is not secret (always 64 hex chars).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, Default)]
pub struct TokenRegistry {
    entries: Vec<(Token, String)>,
}

impl TokenRegistry {
    /// New token for `pane`; any earlier token of that pane (e.g. before "Run again") is revoked.
    pub fn issue(&mut self, pane: &str) -> Result<Token, getrandom::Error> {
        let token = Token::generate()?;
        self.revoke(pane);
        self.entries.push((token.clone(), pane.to_owned()));
        Ok(token)
    }

    /// Pane id owning `presented`, or `None`. Every entry is compared so timing does not reveal which one matched.
    /// ponytail: linear scan, fine for the few dozen panes an app holds.
    pub fn resolve(&self, presented: &str) -> Option<&str> {
        let mut found = None;
        for (token, pane) in &self.entries {
            if ct_eq(token.0.as_bytes(), presented.as_bytes()) {
                found = Some(pane.as_str());
            }
        }
        found
    }

    pub fn revoke(&mut self, pane: &str) {
        self.entries.retain(|(_, p)| p != pane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_64_hex_and_unique() {
        let a = Token::generate().unwrap();
        let b = Token::generate().unwrap();
        assert_eq!(a.expose().len(), 64);
        assert!(a.expose().bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.expose(), b.expose());
    }

    #[test]
    fn debug_does_not_leak() {
        let mut reg = TokenRegistry::default();
        let t = reg.issue("p1").unwrap();
        assert!(!format!("{t:?}").contains(t.expose()));
        assert!(!format!("{reg:?}").contains(t.expose()));
    }

    #[test]
    fn resolve_and_revoke() {
        let mut reg = TokenRegistry::default();
        let t1 = reg.issue("p1").unwrap();
        let t2 = reg.issue("p2").unwrap();
        assert_eq!(reg.resolve(t1.expose()), Some("p1"));
        assert_eq!(reg.resolve(t2.expose()), Some("p2"));
        assert_eq!(reg.resolve(""), None);
        assert_eq!(reg.resolve(&t1.expose()[..63]), None);
        assert_eq!(reg.resolve(&t1.expose().to_uppercase()), None);
        reg.revoke("p1");
        assert_eq!(reg.resolve(t1.expose()), None);
        assert_eq!(reg.resolve(t2.expose()), Some("p2"));
    }

    #[test]
    fn reissue_revokes_old_token() {
        let mut reg = TokenRegistry::default();
        let old = reg.issue("p1").unwrap();
        let new = reg.issue("p1").unwrap();
        assert_eq!(reg.resolve(old.expose()), None);
        assert_eq!(reg.resolve(new.expose()), Some("p1"));
    }

    #[test]
    fn ct_eq_basic() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
