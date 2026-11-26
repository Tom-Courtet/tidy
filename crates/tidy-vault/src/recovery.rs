//! Clé de récupération : 120 bits aléatoires, écrits en 24 caractères (base 32 de
//! Crockford : ni `I`, ni `L`, ni `O`, ni `U`), par groupes de quatre :
//! `7K3M-QX9P-…`. Affichée une seule fois à la création du coffre ; elle remplace le
//! courriel de récupération de l'original (pas de serveur).

use std::fmt;

use zeroize::Zeroizing;

use crate::{Result, VaultError, crypto};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Nombre de caractères significatifs.
pub const LEN: usize = 24;
const GROUP: usize = 4;

/// Clé de récupération (forme canonique : 24 caractères, sans tirets). Effacée de la
/// mémoire à sa libération ; jamais affichée par `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryKey(Zeroizing<String>);

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryKey(…)")
    }
}

impl RecoveryKey {
    /// Nouvelle clé, tirée de l'aléa du système.
    pub fn generate() -> Result<Self> {
        let bytes = Zeroizing::new(crypto::random::<15>()?);
        let mut s = Zeroizing::new(String::with_capacity(LEN));
        // 15 octets = 120 bits = 24 symboles de 5 bits.
        let mut acc: u32 = 0;
        let mut bits = 0;
        for b in bytes.iter() {
            acc = (acc << 8) | u32::from(*b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                s.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
            }
            acc &= (1 << bits) - 1;
        }
        Ok(Self(s))
    }

    /// Lit une clé saisie : casse, espaces et tirets indifférents ; `O` vaut `0`, `I`
    /// et `L` valent `1`.
    pub fn parse(text: &str) -> Result<Self> {
        let mut s = Zeroizing::new(String::with_capacity(LEN));
        for c in text.chars() {
            let c = match c.to_ascii_uppercase() {
                ' ' | '-' | '\t' => continue,
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            if !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
                return Err(VaultError::BadRecoveryKey);
            }
            s.push(c);
        }
        if s.len() != LEN {
            return Err(VaultError::BadRecoveryKey);
        }
        Ok(Self(s))
    }

    /// Forme à recopier : `XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`.
    pub fn display(&self) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::with_capacity(LEN + LEN / GROUP));
        for (i, c) in self.0.chars().enumerate() {
            if i > 0 && i % GROUP == 0 {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_parse_back() {
        let a = RecoveryKey::generate().unwrap();
        let b = RecoveryKey::generate().unwrap();
        assert_ne!(a, b);
        let shown = a.display();
        assert_eq!(shown.len(), LEN + LEN / GROUP - 1);
        assert_eq!(RecoveryKey::parse(&shown).unwrap(), a);
        // Saisie approximative : minuscules, espaces, O pour 0.
        let typed = shown.to_lowercase().replace('-', " ").replace('0', "o");
        assert_eq!(RecoveryKey::parse(&typed).unwrap(), a);
        assert!(!format!("{a:?}").contains(&shown[..4]));
    }

    #[test]
    fn bad_keys_are_refused() {
        assert!(RecoveryKey::parse("").is_err());
        assert!(RecoveryKey::parse("ABCD").is_err());
        assert!(RecoveryKey::parse(&"U".repeat(LEN)).is_err());
        assert!(RecoveryKey::parse(&"é".repeat(LEN)).is_err());
        assert!(RecoveryKey::parse(&"A".repeat(LEN)).is_ok());
    }
}
