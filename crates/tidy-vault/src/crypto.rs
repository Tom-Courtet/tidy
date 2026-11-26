//! Primitives cryptographiques du coffre : aléa, dérivation de clé (Argon2id),
//! enveloppes (XChaCha20-Poly1305) et chiffrement d'un flux par morceaux.
//!
//! Format d'un objet chiffré (un fichier du coffre) :
//!
//! ```text
//! "TIDYOBJ1" (8 octets) · préfixe de nonce aléatoire (19 octets)
//! morceau 0 · morceau 1 · … · dernier morceau
//! ```
//!
//! Chaque morceau chiffre au plus [`CHUNK`] octets et porte son étiquette (16 octets).
//! Son nonce est `préfixe ‖ numéro (u32, gros-boutiste) ‖ drapeau « dernier »` : les
//! morceaux ne peuvent être ni permutés, ni retirés, ni tronqués sans que le
//! déchiffrement échoue. Les données associées sont l'identifiant de l'objet : un objet
//! ne peut pas passer pour un autre. Le dernier morceau est toujours plus court que
//! [`CHUNK`] (au besoin vide), ce qui le reconnaît à la lecture.

use std::io::{self, Read, Write};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{Result, VaultError};

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
pub const SALT_LEN: usize = 16;
/// Taille d'un morceau en clair.
pub const CHUNK: usize = 1 << 20;
const OBJECT_MAGIC: &[u8; 8] = b"TIDYOBJ1";
const PREFIX_LEN: usize = NONCE_LEN - 5;

/// Clé de 32 octets, effacée de la mémoire à sa libération.
pub type Key = Zeroizing<[u8; KEY_LEN]>;

/// Réglages d'Argon2id, enregistrés dans l'en-tête du coffre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Mémoire, en Kio.
    pub m_kib: u32,
    /// Nombre de passes.
    pub t: u32,
    /// Parallélisme.
    pub p: u32,
}

impl Default for KdfParams {
    /// 64 Mio, 3 passes : environ une demi-seconde sur un PC courant.
    fn default() -> Self {
        Self {
            m_kib: 64 * 1024,
            t: 3,
            p: 1,
        }
    }
}

/// Octets aléatoires du système.
pub fn random<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| VaultError::Random(e.to_string()))?;
    Ok(buf)
}

pub fn random_key() -> Result<Key> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    getrandom::fill(key.as_mut()).map_err(|e| VaultError::Random(e.to_string()))?;
    Ok(key)
}

/// Clé dérivée d'un secret (mot de passe, clé de récupération) par Argon2id.
pub fn derive(secret: &[u8], salt: &[u8], params: &KdfParams) -> Result<Key> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let p = Params::new(params.m_kib, params.t, params.p, Some(KEY_LEN))
        .map_err(|e| VaultError::Corrupt(format!("réglages d'Argon2 : {e}")))?;
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(secret, salt, out.as_mut())
        .map_err(|e| VaultError::Corrupt(format!("Argon2 : {e}")))?;
    Ok(out)
}

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

/// Chiffre `plaintext` : `nonce ‖ texte chiffré ‖ étiquette`.
pub fn seal(key: &[u8; KEY_LEN], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let nonce = random::<NONCE_LEN>()?;
    let ct = cipher(key)
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VaultError::Corrupt("chiffrement impossible".into()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Déchiffre ce que [`seal`] a produit. `None` : mauvaise clé ou données altérées.
pub fn open(key: &[u8; KEY_LEN], aad: &[u8], sealed: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    cipher(key)
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .ok()
        .map(Zeroizing::new)
}

fn chunk_nonce(prefix: &[u8; PREFIX_LEN], counter: u32, last: bool) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[..PREFIX_LEN].copy_from_slice(prefix);
    n[PREFIX_LEN..PREFIX_LEN + 4].copy_from_slice(&counter.to_be_bytes());
    n[NONCE_LEN - 1] = u8::from(last);
    n
}

/// Lit jusqu'à remplir `buf` ou atteindre la fin. Rend le nombre d'octets lus.
pub fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Chiffre tout `src` dans `dst` (format en tête de module). Rend la taille en clair.
pub fn encrypt_stream(
    key: &[u8; KEY_LEN],
    id: &[u8],
    src: &mut impl Read,
    dst: &mut impl Write,
) -> Result<u64> {
    let prefix = random::<PREFIX_LEN>()?;
    dst.write_all(OBJECT_MAGIC)
        .map_err(VaultError::io("écriture"))?;
    dst.write_all(&prefix).map_err(VaultError::io("écriture"))?;
    let c = cipher(key);
    let mut buf = Zeroizing::new(vec![0u8; CHUNK]);
    let mut counter: u32 = 0;
    let mut total = 0u64;
    loop {
        let n = read_full(src, &mut buf).map_err(VaultError::io("lecture du fichier"))?;
        let last = n < CHUNK;
        let nonce = chunk_nonce(&prefix, counter, last);
        let ct = c
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &buf[..n],
                    aad: id,
                },
            )
            .map_err(|_| VaultError::Corrupt("chiffrement impossible".into()))?;
        dst.write_all(&ct).map_err(VaultError::io("écriture"))?;
        total += n as u64;
        if last {
            return Ok(total);
        }
        counter = counter
            .checked_add(1)
            .ok_or_else(|| VaultError::Unsupported("fichier trop gros".into()))?;
    }
}

/// Déchiffre un objet et passe chaque morceau en clair à `sink`. Échoue si l'objet a
/// été altéré, tronqué ou prolongé.
pub fn decrypt_stream(
    key: &[u8; KEY_LEN],
    id: &[u8],
    src: &mut impl Read,
    sink: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<u64> {
    let corrupt = || VaultError::Corrupt("fichier chiffré altéré".into());
    let mut head = [0u8; 8 + PREFIX_LEN];
    let n = read_full(src, &mut head).map_err(VaultError::io("lecture du coffre"))?;
    if n < head.len() || &head[..8] != OBJECT_MAGIC {
        return Err(corrupt());
    }
    let mut prefix = [0u8; PREFIX_LEN];
    prefix.copy_from_slice(&head[8..]);
    let c = cipher(key);
    let mut buf = vec![0u8; CHUNK + TAG_LEN];
    let mut counter: u32 = 0;
    let mut total = 0u64;
    loop {
        let n = read_full(src, &mut buf).map_err(VaultError::io("lecture du coffre"))?;
        if n < TAG_LEN {
            // Fin atteinte sans dernier morceau : tronqué.
            return Err(corrupt());
        }
        let last = n < CHUNK + TAG_LEN;
        let nonce = chunk_nonce(&prefix, counter, last);
        let plain = Zeroizing::new(
            c.decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &buf[..n],
                    aad: id,
                },
            )
            .map_err(|_| corrupt())?,
        );
        sink(&plain)?;
        total += plain.len() as u64;
        if last {
            // Rien après le dernier morceau.
            let mut extra = [0u8; 1];
            if read_full(src, &mut extra).map_err(VaultError::io("lecture du coffre"))? != 0 {
                return Err(corrupt());
            }
            return Ok(total);
        }
        counter = counter.checked_add(1).ok_or_else(corrupt)?;
    }
}

/// Hexadécimal (en-tête du coffre, identifiants).
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 15)] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(len: usize) {
        let key = random_key().unwrap();
        let data: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
        let mut enc = Vec::new();
        let n = encrypt_stream(&key, b"id", &mut data.as_slice(), &mut enc).unwrap();
        assert_eq!(n, len as u64);
        let mut out = Vec::new();
        decrypt_stream(&key, b"id", &mut enc.as_slice(), &mut |c| {
            out.extend_from_slice(c);
            Ok(())
        })
        .unwrap();
        assert_eq!(out, data);
        // Autre identifiant : refusé.
        assert!(decrypt_stream(&key, b"autre", &mut enc.as_slice(), &mut |_| Ok(())).is_err());
    }

    #[test]
    fn streams_roundtrip_at_chunk_boundaries() {
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK] {
            roundtrip(len);
        }
    }

    #[test]
    fn truncation_and_extension_are_detected() {
        let key = random_key().unwrap();
        let data = vec![5u8; 2 * CHUNK + 10];
        let mut enc = Vec::new();
        encrypt_stream(&key, b"id", &mut data.as_slice(), &mut enc).unwrap();
        let ok =
            |bytes: &[u8]| decrypt_stream(&key, b"id", &mut &bytes[..], &mut |_| Ok(())).is_ok();
        assert!(ok(&enc));
        // Coupé juste après un morceau complet.
        let head = 8 + PREFIX_LEN;
        assert!(!ok(&enc[..head + CHUNK + TAG_LEN]));
        assert!(!ok(&enc[..enc.len() - 1]));
        let mut longer = enc.clone();
        longer.push(0);
        assert!(!ok(&longer));
        let mut flipped = enc.clone();
        flipped[head + 3] ^= 1;
        assert!(!ok(&flipped));
    }

    #[test]
    fn seal_and_hex() {
        let key = random_key().unwrap();
        let sealed = seal(&key, b"aad", b"secret").unwrap();
        assert_eq!(&open(&key, b"aad", &sealed).unwrap()[..], b"secret");
        assert!(open(&key, b"autre", &sealed).is_none());
        let other = random_key().unwrap();
        assert!(open(&other, b"aad", &sealed).is_none());
        assert_eq!(unhex(&hex(&sealed)).unwrap(), sealed);
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
    }
}
