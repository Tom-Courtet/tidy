//! Cache de l'index sur le disque (`search/index.bin`) : au démarrage, les résultats
//! sont là tout de suite, avant la fin du parcours des dossiers.
//!
//! Format binaire simple, petit-boutiste : en-tête, empreinte de la configuration
//! ([`crate::Config::signature`]), dossiers, éléments. Un fichier illisible, d'une autre
//! version ou d'une autre configuration est ignoré (l'index est reconstruit).

use std::path::Path;

use anyhow::{Context, Result, bail};
use tidy_core::category::Category;

use crate::config::Root;
use crate::index::{Dir, Entry, Index};

const MAGIC: &[u8; 8] = b"TIDYIDX1";

/// Écrit `index` dans `path` (fichier temporaire puis renommage).
pub fn save(index: &Index, signature: u64, path: &Path) -> Result<()> {
    let mut out = Vec::with_capacity(index.entries.len() * 40 + 1024);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&signature.to_le_bytes());
    put_u32(&mut out, index.dirs.len() as u32);
    for dir in &index.dirs {
        out.push(dir.flags);
        out.push(u8::from(dir.alive));
        put_str(&mut out, &dir.path);
    }
    put_u32(&mut out, index.entries.len() as u32);
    for e in &index.entries {
        put_u32(&mut out, e.dir);
        out.push(category_code(e.category));
        out.push(u8::from(e.is_dir));
        out.extend_from_slice(&e.modified.to_le_bytes());
        put_str(&mut out, &e.name);
    }
    tidy_core::store::write_atomic(path, &out)
}

/// Lit le cache, s'il correspond à `signature`.
pub fn load(path: &Path, signature: u64, roots: Vec<Root>) -> Result<Index> {
    let bytes = std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?;
    let mut r = Reader {
        bytes: &bytes,
        at: 0,
    };
    if r.take(8)? != MAGIC {
        bail!("format inconnu");
    }
    if r.u64()? != signature {
        bail!("autre configuration");
    }
    let mut index = Index::new(roots);
    let dirs = r.u32()? as usize;
    for id in 0..dirs {
        let flags = r.u8()?;
        let alive = r.u8()? != 0;
        let path: Box<str> = r.str()?.into();
        if alive {
            index.dir_ids.insert(path.clone(), id as u32);
        }
        index.dirs.push(Dir { path, flags, alive });
    }
    let entries = r.u32()? as usize;
    index
        .entries
        .reserve(entries.min(crate::index::MAX_ENTRIES));
    for _ in 0..entries {
        let dir = r.u32()?;
        let category = Category::ALL
            .get(r.u8()? as usize)
            .copied()
            .unwrap_or(Category::Other);
        let is_dir = r.u8()? != 0;
        let modified = i64::from_le_bytes(r.take(8)?.try_into()?);
        let name = r.str()?;
        if dir as usize >= dirs {
            bail!("dossier inconnu");
        }
        index
            .entries
            .push(Entry::new(name, dir, category, is_dir, modified));
    }
    Ok(index)
}

fn category_code(category: Category) -> u8 {
    Category::ALL
        .iter()
        .position(|c| *c == category)
        .unwrap_or(Category::ALL.len() - 1) as u8
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).context("taille")?;
        let slice = self.bytes.get(self.at..end).context("fichier tronqué")?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }

    fn str(&mut self) -> Result<&'a str> {
        let len = self.u32()? as usize;
        Ok(std::str::from_utf8(self.take(len)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{Query, search};

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("search/index.bin");
        let mut index = Index::new(Vec::new());
        index.push_synthetic("/a", "Été.jpg", false, 42);
        index.push_synthetic("/a/b", "Rapports", true, 7);
        save(&index, 99, &path).unwrap();
        assert!(load(&path, 98, Vec::new()).is_err(), "autre configuration");
        let back = load(&path, 99, Vec::new()).unwrap();
        assert_eq!(back.len(), 2);
        let q = Query {
            text: "ete".into(),
            limit: 10,
            ..Query::default()
        };
        let hits = search(&back, &q);
        assert_eq!(hits[0].path, std::path::Path::new("/a/Été.jpg"));
        assert_eq!(hits[0].modified, 42);
        assert_eq!(hits[0].category, Category::Image);
        // Fichier abîmé : erreur, pas de panique.
        std::fs::write(&path, &b"TIDYIDX1\x63\0\0\0\0\0\0\0\xff\xff"[..]).unwrap();
        assert!(load(&path, 99, Vec::new()).is_err());
    }
}
