//! Coffre chiffré fichier par fichier, dans un dossier (`<données>/vault`) :
//!
//! ```text
//! vault.json        en-tête : réglages d'Argon2id, clé maîtresse enveloppée deux fois
//! index.tv          index chiffré (noms, tailles, dates, objets)
//! objects/<id>.tv   un fichier chiffré par fichier du coffre
//! open.noindex/     copies en clair des fichiers ouverts (effacées au verrouillage ;
//!                   le suffixe `.noindex` écarte le dossier de Spotlight)
//! staging/          copies de passage vers la corbeille
//! ```
//!
//! Ordre des écritures, pour ne jamais perdre un fichier :
//! - ajout : objets chiffrés et vérifiés → index → corbeille de l'original (en cas
//!   d'échec de la corbeille, l'ajout est annulé) ;
//! - extraction : copie en clair complète, renommée à sa place → index → objets ;
//! - fichier ouvert modifié : nouvel objet vérifié → index → ancien objet.
//!
//! Un objet que l'index ne cite pas vient donc toujours d'une opération interrompue
//! dont l'original existe encore : il est supprimé au déverrouillage suivant.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::{self, KdfParams, Key};
use crate::fsx::{self, Found};
use crate::{
    Dispose, LockReport, RecoveryKey, Result, VaultBackend, VaultError, VaultItem, check_password,
};

const HEADER: &str = "vault.json";
const INDEX: &str = "index.tv";
const OBJECTS: &str = "objects";
const OPEN: &str = "open.noindex";
const STAGING: &str = "staging";
const INDEX_MAGIC: &[u8; 8] = b"TIDYIDX1";
const AAD_INDEX: &[u8] = b"tidy-vault/index/v1";
const AAD_PASSWORD: &[u8] = b"tidy-vault/key/password/v1";
const AAD_RECOVERY: &[u8] = b"tidy-vault/key/recovery/v1";
const VERSION: u32 = 1;

/// Une enveloppe de la clé maîtresse.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Slot {
    salt: String,
    /// `nonce ‖ clé chiffrée ‖ étiquette`, en hexadécimal.
    sealed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Header {
    version: u32,
    kdf: KdfParams,
    password: Slot,
    recovery: Slot,
}

/// Une entrée de l'index : un fichier ou un dossier, désigné par son chemin dans le
/// coffre (composants séparés par `/`, le premier est l'élément de premier niveau).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    path: String,
    is_dir: bool,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    modified: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Index {
    version: u32,
    entries: Vec<Entry>,
}

impl Index {
    fn erase(&mut self) {
        for e in &mut self.entries {
            e.path.zeroize();
            if let Some(o) = e.object.as_mut() {
                o.zeroize();
            }
        }
        self.entries.clear();
    }
}

/// `path` est l'élément `name` ou l'un de ses descendants.
fn in_item(path: &str, name: &str) -> bool {
    path == name
        || path
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Taille et date d'une copie ouverte au moment où Tidy l'a écrite ou relue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileState {
    size: u64,
    modified: Option<SystemTime>,
}

impl FileState {
    fn of(meta: &fs::Metadata) -> Self {
        Self {
            size: meta.len(),
            modified: meta.modified().ok(),
        }
    }
}

/// Un élément déchiffré pour être ouvert.
#[derive(Debug, Clone)]
struct Opened {
    /// `open.noindex/<jeton>`.
    temp: PathBuf,
    /// Nom de l'élément dans `temp` (celui qu'il avait à l'ouverture).
    temp_name: String,
    /// États connus, par chemin relatif dans l'élément (`""` : l'élément lui-même).
    /// Sans état connu (copie reprise d'une session interrompue), le contenu est
    /// comparé à celui du coffre.
    states: HashMap<String, FileState>,
}

/// Panne simulée (tests).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// Arrêt après l'écriture des objets, avant l'index.
    BeforeIndex,
    /// Un octet de l'objet change entre l'écriture et la vérification.
    CorruptObject,
}

/// Coffre chiffré fichier par fichier (voir le module).
pub struct FileVault {
    root: PathBuf,
    params: KdfParams,
    key: Option<Key>,
    index: Index,
    opened: HashMap<String, Opened>,
    #[cfg(test)]
    pub(crate) fault: Option<Fault>,
}

impl std::fmt::Debug for FileVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileVault")
            .field("root", &self.root)
            .field("unlocked", &self.key.is_some())
            .finish_non_exhaustive()
    }
}

fn unix_secs(t: SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
}

impl FileVault {
    /// Coffre du dossier `root` (créé au besoin à la création du coffre).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_params(root, KdfParams::default())
    }

    /// Même chose avec d'autres réglages d'Argon2id pour un nouveau coffre (tests).
    pub fn with_params(root: impl Into<PathBuf>, params: KdfParams) -> Self {
        Self {
            root: root.into(),
            params,
            key: None,
            index: Index::default(),
            opened: HashMap::new(),
            #[cfg(test)]
            fault: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Un coffre existe dans `root` (sans rien déchiffrer).
    pub fn exists_in(root: &Path) -> bool {
        root.join(HEADER).is_file()
    }

    /// Des copies en clair restent dans le dossier temporaire (session interrompue, ou
    /// fichier encore ouvert au verrouillage).
    pub fn has_leftovers(&self) -> bool {
        fs::read_dir(self.root.join(OPEN)).is_ok_and(|mut d| d.next().is_some())
    }

    fn key(&self) -> Result<Key> {
        self.key.clone().ok_or(VaultError::Locked)
    }

    fn object_path(&self, id: &str) -> PathBuf {
        self.root.join(OBJECTS).join(format!("{id}.tv"))
    }

    // --- En-tête et index ---

    fn read_header(&self) -> Result<Header> {
        let bytes = match fs::read(self.root.join(HEADER)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(VaultError::NotCreated);
            }
            Err(e) => return Err(VaultError::io("lecture de l'en-tête")(e)),
        };
        let header: Header = serde_json::from_slice(&bytes)
            .map_err(|e| VaultError::Corrupt(format!("en-tête : {e}")))?;
        if header.version > VERSION {
            return Err(VaultError::Unsupported(
                "coffre créé par une version plus récente de Tidy".into(),
            ));
        }
        Ok(header)
    }

    fn write_header(&self, header: &Header) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(header)
            .map_err(|e| VaultError::Corrupt(format!("en-tête : {e}")))?;
        fsx::write_atomic(&self.root.join(HEADER), &bytes)
            .map_err(VaultError::io("écriture de l'en-tête"))
    }

    fn make_slot(secret: &[u8], params: &KdfParams, aad: &[u8], master: &Key) -> Result<Slot> {
        let salt = crypto::random::<{ crypto::SALT_LEN }>()?;
        let kek = crypto::derive(secret, &salt, params)?;
        Ok(Slot {
            salt: crypto::hex(&salt),
            sealed: crypto::hex(&crypto::seal(&kek, aad, master.as_ref())?),
        })
    }

    /// Clé maîtresse ouverte par `secret`, `None` si le secret est faux.
    fn open_slot(
        slot: &Slot,
        secret: &[u8],
        params: &KdfParams,
        aad: &[u8],
    ) -> Result<Option<Key>> {
        let corrupt = || VaultError::Corrupt("enveloppe de clé".into());
        let salt = crypto::unhex(&slot.salt).ok_or_else(corrupt)?;
        let sealed = crypto::unhex(&slot.sealed).ok_or_else(corrupt)?;
        let kek = crypto::derive(secret, &salt, params)?;
        let Some(plain) = crypto::open(&kek, aad, &sealed) else {
            return Ok(None);
        };
        let mut key = Zeroizing::new([0u8; crypto::KEY_LEN]);
        if plain.len() != key.len() {
            return Err(corrupt());
        }
        key.copy_from_slice(&plain);
        Ok(Some(key))
    }

    fn read_index(&self, key: &Key) -> Result<Index> {
        let bytes = fs::read(self.root.join(INDEX)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VaultError::Corrupt("index manquant".into())
            } else {
                VaultError::io("lecture de l'index")(e)
            }
        })?;
        if bytes.len() < INDEX_MAGIC.len() || &bytes[..INDEX_MAGIC.len()] != INDEX_MAGIC {
            return Err(VaultError::Corrupt("index altéré".into()));
        }
        let plain = crypto::open(key, AAD_INDEX, &bytes[INDEX_MAGIC.len()..])
            .ok_or_else(|| VaultError::Corrupt("index altéré".into()))?;
        serde_json::from_slice(&plain).map_err(|e| VaultError::Corrupt(format!("index : {e}")))
    }

    fn write_index(&self, key: &Key, index: &Index) -> Result<()> {
        let json = Zeroizing::new(
            serde_json::to_vec(index).map_err(|e| VaultError::Corrupt(format!("index : {e}")))?,
        );
        let mut bytes = INDEX_MAGIC.to_vec();
        bytes.extend(crypto::seal(key, AAD_INDEX, &json)?);
        fsx::write_atomic(&self.root.join(INDEX), &bytes)
            .map_err(VaultError::io("écriture de l'index"))
    }

    /// Fin commune des déverrouillages : index, ménage, reprise des copies laissées.
    fn finish_unlock(&mut self, key: Key) -> Result<()> {
        let index = self.read_index(&key)?;
        self.key = Some(key);
        self.index = index;
        self.opened.clear();
        self.remove_orphans();
        self.recover_leftovers();
        Ok(())
    }

    /// Supprime les objets chiffrés que l'index ne cite pas (opérations interrompues,
    /// voir le module) et les fichiers de passage.
    fn remove_orphans(&self) {
        let used: HashSet<&str> = self
            .index
            .entries
            .iter()
            .filter_map(|e| e.object.as_deref())
            .collect();
        if let Ok(dir) = fs::read_dir(self.root.join(OBJECTS)) {
            for e in dir.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let keep = name.strip_suffix(".tv").is_some_and(|id| used.contains(id));
                if !keep {
                    log::info!("coffre : objet orphelin supprimé");
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        // Copies de passage vers la corbeille : écrasées.
        let staging = self.root.join(STAGING);
        if staging.exists() {
            let _ = fsx::wipe_tree(&staging);
        }
    }

    /// Reprend les copies en clair d'une session interrompue : les modifications sont
    /// rechiffrées (comparaison du contenu), un élément disparu du coffre y revient,
    /// puis la copie est effacée. En cas d'échec, la copie reste.
    fn recover_leftovers(&mut self) {
        let Ok(dir) = fs::read_dir(self.root.join(OPEN)) else {
            return;
        };
        let tokens: Vec<PathBuf> = dir.flatten().map(|e| e.path()).collect();
        for temp in tokens {
            let children: Vec<_> = fs::read_dir(&temp)
                .map(|d| d.flatten().collect())
                .unwrap_or_default();
            if children.is_empty() {
                let _ = fs::remove_dir(&temp);
                continue;
            }
            for child in children {
                let temp_name = child.file_name().to_string_lossy().into_owned();
                let known = self.top_names().any(|n| n == temp_name);
                if known && !self.opened.contains_key(&temp_name) {
                    self.opened.insert(
                        temp_name.clone(),
                        Opened {
                            temp: temp.clone(),
                            temp_name: temp_name.clone(),
                            states: HashMap::new(),
                        },
                    );
                    match self.sync_item(&temp_name) {
                        Ok(n) => {
                            log::info!("coffre : copie reprise ({n} fichier(s) rechiffré(s))");
                            self.forget_opened(&temp_name);
                        }
                        Err(e) => {
                            log::warn!("coffre : copie non reprise : {e}");
                            self.opened.remove(&temp_name);
                        }
                    }
                } else {
                    // Élément retiré du coffre entre-temps : il y revient.
                    let path = child.path();
                    let mut wipe = |p: &Path| {
                        let kept = fsx::wipe_tree(p);
                        if !kept.is_empty() {
                            log::warn!("coffre : {} copie(s) non effacée(s)", kept.len());
                        }
                        // Toujours « réussi » : la copie est déjà dans le coffre, et
                        // annuler l'ajout après un effacement partiel la perdrait.
                        Ok::<(), std::io::Error>(())
                    };
                    if let Err(e) = self.add(&path, &mut wipe) {
                        log::warn!("coffre : copie non reprise : {e}");
                    }
                }
            }
            let _ = fs::remove_dir(&temp);
        }
    }

    fn top_names(&self) -> impl Iterator<Item = &str> {
        self.index
            .entries
            .iter()
            .filter(|e| !e.path.contains('/'))
            .map(|e| e.path.as_str())
    }

    fn top_taken(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        self.top_names().any(|n| n.to_lowercase() == lower)
    }

    fn item_entries(&self, name: &str) -> Result<Vec<Entry>> {
        let mut entries: Vec<Entry> = self
            .index
            .entries
            .iter()
            .filter(|e| in_item(&e.path, name))
            .cloned()
            .collect();
        if !entries.iter().any(|e| e.path == name) {
            return Err(VaultError::NotFound(name.to_owned()));
        }
        // Parents avant enfants.
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    // --- Objets ---

    /// Chiffre `source` dans un nouvel objet, puis le relit et le déchiffre en entier
    /// pour le comparer à `source`. L'identifiant est ajouté à `created` dès le début
    /// (pour le ménage en cas d'échec).
    fn store_file(
        &self,
        key: &Key,
        source: &Path,
        created: &mut Vec<String>,
    ) -> Result<(String, u64)> {
        let id = crypto::hex(&crypto::random::<16>()?);
        created.push(id.clone());
        let objects = self.root.join(OBJECTS);
        fs::create_dir_all(&objects).map_err(VaultError::io("dossier du coffre"))?;
        let part = objects.join(format!("{id}.part"));
        let target = self.object_path(&id);
        let mut src =
            BufReader::new(File::open(source).map_err(VaultError::io("lecture du fichier"))?);
        let file = File::create(&part).map_err(VaultError::io("écriture du coffre"))?;
        let mut dst = BufWriter::new(file);
        let size = crypto::encrypt_stream(key, id.as_bytes(), &mut src, &mut dst)?;
        let file = dst
            .into_inner()
            .map_err(|e| VaultError::io("écriture du coffre")(e.into_error()))?;
        file.sync_all()
            .map_err(VaultError::io("écriture du coffre"))?;
        drop(file);
        fs::rename(&part, &target).map_err(VaultError::io("écriture du coffre"))?;
        #[cfg(test)]
        if self.fault == Some(Fault::CorruptObject) {
            let mut bytes = fs::read(&target).unwrap();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            fs::write(&target, bytes).unwrap();
        }
        self.verify(key, &id, source)?;
        Ok((id, size))
    }

    /// Relit l'objet `id`, le déchiffre en entier et le compare à `source`.
    fn verify(&self, key: &Key, id: &str, source: &Path) -> Result<()> {
        let mismatch = || VaultError::Verification(String::from("le contenu relu diffère"));
        let mut obj = BufReader::new(
            File::open(self.object_path(id)).map_err(VaultError::io("relecture du coffre"))?,
        );
        let mut src =
            BufReader::new(File::open(source).map_err(VaultError::io("relecture du fichier"))?);
        let mut expected = Zeroizing::new(Vec::new());
        crypto::decrypt_stream(key, id.as_bytes(), &mut obj, &mut |chunk| {
            expected.resize(chunk.len(), 0);
            let n = crypto::read_full(&mut src, &mut expected)
                .map_err(VaultError::io("relecture du fichier"))?;
            if n != chunk.len() || expected[..] != *chunk {
                return Err(mismatch());
            }
            Ok(())
        })
        .map_err(|e| match e {
            VaultError::Corrupt(_) => VaultError::Verification("fichier chiffré illisible".into()),
            e => e,
        })?;
        let mut extra = [0u8; 1];
        if crypto::read_full(&mut src, &mut extra)
            .map_err(VaultError::io("relecture du fichier"))?
            != 0
        {
            return Err(mismatch());
        }
        Ok(())
    }

    /// Le contenu de l'objet `id` est égal à celui de `path`.
    fn same_content(&self, key: &Key, id: &str, path: &Path) -> bool {
        self.verify(key, id, path).is_ok()
    }

    fn delete_objects(&self, ids: &[String]) {
        for id in ids {
            let _ = fs::remove_file(self.object_path(id));
            let _ = fs::remove_file(self.root.join(OBJECTS).join(format!("{id}.part")));
        }
    }

    /// Écrit l'élément `name` en clair à `dest` (qui ne doit pas exister).
    fn write_item(&self, key: &Key, name: &str, entries: &[Entry], dest: &Path) -> Result<()> {
        for e in entries {
            let rest = e.path.strip_prefix(name).unwrap_or_default();
            let mut target = dest.to_path_buf();
            for part in rest.split('/').filter(|p| !p.is_empty()) {
                // Un seul composant ordinaire (ni `..`, ni racine, ni séparateur du
                // système) : on n'écrit jamais hors de `dest`.
                let mut parts = Path::new(part).components();
                if !matches!(
                    (parts.next(), parts.next()),
                    (Some(std::path::Component::Normal(_)), None)
                ) {
                    return Err(VaultError::Corrupt("chemin refusé dans l'index".into()));
                }
                target.push(part);
            }
            if e.is_dir {
                fs::create_dir_all(&target).map_err(VaultError::io("écriture en clair"))?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(VaultError::io("écriture en clair"))?;
            }
            let id = e
                .object
                .as_deref()
                .ok_or_else(|| VaultError::Corrupt("fichier sans contenu".into()))?;
            let mut obj = BufReader::new(
                File::open(self.object_path(id)).map_err(VaultError::io("lecture du coffre"))?,
            );
            let mut out =
                BufWriter::new(File::create(&target).map_err(VaultError::io("écriture en clair"))?);
            crypto::decrypt_stream(key, id.as_bytes(), &mut obj, &mut |chunk| {
                out.write_all(chunk)
                    .map_err(VaultError::io("écriture en clair"))
            })?;
            let file = out
                .into_inner()
                .map_err(|e| VaultError::io("écriture en clair")(e.into_error()))?;
            if let Some(m) = e.modified.and_then(|s| u64::try_from(s).ok()) {
                let _ = file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(m));
            }
            file.sync_all()
                .map_err(VaultError::io("écriture en clair"))?;
        }
        Ok(())
    }

    // --- Fichiers ouverts ---

    /// Rechiffre ce qui a changé dans la copie ouverte de `name`. Rend le nombre de
    /// fichiers rechiffrés. Les fichiers supprimés de la copie restent dans le coffre.
    fn sync_item(&mut self, name: &str) -> Result<usize> {
        let key = self.key()?;
        let Some(opened) = self.opened.get(name).cloned() else {
            return Ok(0);
        };
        let item = opened.temp.join(&opened.temp_name);
        let Ok(meta) = fs::symlink_metadata(&item) else {
            return Ok(0);
        };
        let mut found = vec![Found {
            path: item.clone(),
            rel: String::new(),
            is_dir: meta.is_dir(),
            is_symlink: meta.file_type().is_symlink(),
            meta,
        }];
        if found[0].is_dir {
            found.extend(fsx::walk(&item).map_err(VaultError::io("lecture de la copie"))?);
        }
        let mut index = self.index.clone();
        let mut created = Vec::new();
        let mut replaced = Vec::new();
        let mut states = opened.states.clone();
        let mut updated = 0;
        let result: Result<()> = (|| {
            for f in found.iter().filter(|f| !f.is_symlink) {
                let path = if f.rel.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}/{}", f.rel)
                };
                let pos = index.entries.iter().position(|e| e.path == path);
                if f.is_dir {
                    if pos.is_none() {
                        index.entries.push(Entry {
                            path,
                            is_dir: true,
                            object: None,
                            size: 0,
                            modified: f.meta.modified().ok().and_then(unix_secs),
                        });
                    }
                    continue;
                }
                let state = FileState::of(&f.meta);
                if states.get(&f.rel) == Some(&state) {
                    continue;
                }
                let existing = pos.and_then(|i| index.entries[i].object.clone());
                if !states.contains_key(&f.rel)
                    && existing
                        .as_deref()
                        .is_some_and(|id| self.same_content(&key, id, &f.path))
                {
                    states.insert(f.rel.clone(), state);
                    continue;
                }
                let (id, size) = self.store_file(&key, &f.path, &mut created)?;
                let entry = Entry {
                    path,
                    is_dir: false,
                    object: Some(id),
                    size,
                    modified: f.meta.modified().ok().and_then(unix_secs),
                };
                match pos {
                    Some(i) => index.entries[i] = entry,
                    None => index.entries.push(entry),
                }
                replaced.extend(existing);
                states.insert(f.rel.clone(), state);
                updated += 1;
            }
            if updated > 0 || index.entries.len() != self.index.entries.len() {
                self.write_index(&key, &index)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.delete_objects(&created);
            return Err(e);
        }
        self.index = index;
        self.delete_objects(&replaced);
        if let Some(o) = self.opened.get_mut(name) {
            o.states = states;
        }
        Ok(updated)
    }

    /// Efface la copie ouverte de `name` (déjà rechiffrée). Rend ce qui est resté.
    fn forget_opened(&mut self, name: &str) -> Vec<PathBuf> {
        let Some(o) = self.opened.remove(name) else {
            return Vec::new();
        };
        let kept = fsx::wipe_tree(&o.temp);
        if kept.is_empty() {
            let _ = fs::remove_dir(&o.temp);
        }
        kept
    }

    fn remove_item(&mut self, key: &Key, name: &str) -> Result<()> {
        let mut index = self.index.clone();
        let objects: Vec<String> = index
            .entries
            .iter()
            .filter(|e| in_item(&e.path, name))
            .filter_map(|e| e.object.clone())
            .collect();
        index.entries.retain(|e| !in_item(&e.path, name));
        self.write_index(key, &index)?;
        self.index = index;
        self.delete_objects(&objects);
        Ok(())
    }

    /// Ajout : objets écrits et vérifiés, puis index. Rend le nouvel index.
    fn add_inner(
        &self,
        key: &Key,
        source: &Path,
        meta: &fs::Metadata,
        name: &str,
        created: &mut Vec<String>,
    ) -> Result<Index> {
        let mtime = |m: &fs::Metadata| m.modified().ok().and_then(unix_secs);
        let mut entries = Vec::new();
        if meta.is_dir() {
            let found = fsx::walk(source).map_err(VaultError::io("lecture du dossier"))?;
            if found.iter().any(|f| f.is_symlink) {
                return Err(VaultError::Symlink);
            }
            entries.push(Entry {
                path: name.to_owned(),
                is_dir: true,
                object: None,
                size: 0,
                modified: mtime(meta),
            });
            for f in found {
                let path = format!("{name}/{}", f.rel);
                if f.is_dir {
                    entries.push(Entry {
                        path,
                        is_dir: true,
                        object: None,
                        size: 0,
                        modified: mtime(&f.meta),
                    });
                } else {
                    let (id, size) = self.store_file(key, &f.path, created)?;
                    entries.push(Entry {
                        path,
                        is_dir: false,
                        object: Some(id),
                        size,
                        modified: mtime(&f.meta),
                    });
                }
            }
        } else {
            let (id, size) = self.store_file(key, source, created)?;
            entries.push(Entry {
                path: name.to_owned(),
                is_dir: false,
                object: Some(id),
                size,
                modified: mtime(meta),
            });
        }
        #[cfg(test)]
        if self.fault == Some(Fault::BeforeIndex) {
            return Err(VaultError::Io {
                context: "panne simulée",
                error: std::io::Error::other("arrêt"),
            });
        }
        let mut index = self.index.clone();
        index.entries.extend(entries);
        self.write_index(key, &index)?;
        Ok(index)
    }
}

impl VaultBackend for FileVault {
    fn exists(&self) -> bool {
        Self::exists_in(&self.root)
    }

    fn is_unlocked(&self) -> bool {
        self.key.is_some()
    }

    fn create(&mut self, password: &str, recovery: &RecoveryKey) -> Result<()> {
        check_password(password)?;
        if self.exists() {
            return Err(VaultError::AlreadyExists);
        }
        fs::create_dir_all(self.root.join(OBJECTS)).map_err(VaultError::io("dossier du coffre"))?;
        let master = crypto::random_key()?;
        let header = Header {
            version: VERSION,
            kdf: self.params,
            password: Self::make_slot(password.as_bytes(), &self.params, AAD_PASSWORD, &master)?,
            recovery: Self::make_slot(recovery.as_bytes(), &self.params, AAD_RECOVERY, &master)?,
        };
        let index = Index {
            version: VERSION,
            entries: Vec::new(),
        };
        // L'index d'abord : l'en-tête marque le coffre comme existant.
        self.write_index(&master, &index)?;
        self.write_header(&header)?;
        self.key = Some(master);
        self.index = index;
        self.opened.clear();
        Ok(())
    }

    fn unlock(&mut self, password: &str) -> Result<()> {
        let header = self.read_header()?;
        let key = Self::open_slot(
            &header.password,
            password.as_bytes(),
            &header.kdf,
            AAD_PASSWORD,
        )?
        .ok_or(VaultError::WrongPassword)?;
        self.finish_unlock(key)
    }

    fn recover(&mut self, recovery: &str, new_password: &str) -> Result<()> {
        check_password(new_password)?;
        let recovery = RecoveryKey::parse(recovery)?;
        let mut header = self.read_header()?;
        let key = Self::open_slot(
            &header.recovery,
            recovery.as_bytes(),
            &header.kdf,
            AAD_RECOVERY,
        )?
        .ok_or(VaultError::WrongRecoveryKey)?;
        // Charger l'index avant de toucher au mot de passe : un coffre illisible reste
        // tel quel.
        self.read_index(&key)?;
        header.password =
            Self::make_slot(new_password.as_bytes(), &header.kdf, AAD_PASSWORD, &key)?;
        self.write_header(&header)?;
        self.finish_unlock(key)
    }

    fn lock(&mut self) -> LockReport {
        let mut report = LockReport::default();
        if self.key.is_some() {
            let names: Vec<String> = self.opened.keys().cloned().collect();
            for name in names {
                match self.sync_item(&name) {
                    Ok(n) => {
                        report.updated += n;
                        report.kept.extend(self.forget_opened(&name));
                    }
                    Err(e) => {
                        log::warn!("coffre : modification non rechiffrée : {e}");
                        if let Some(o) = self.opened.remove(&name) {
                            report.kept.push(o.temp.join(o.temp_name));
                        }
                    }
                }
            }
        }
        self.opened.clear();
        self.key = None;
        self.index.erase();
        report
    }

    fn list(&self) -> Result<Vec<VaultItem>> {
        if self.key.is_none() {
            return Err(VaultError::Locked);
        }
        let mut items: Vec<VaultItem> = self
            .index
            .entries
            .iter()
            .filter(|e| !e.path.contains('/'))
            .map(|e| VaultItem {
                name: e.path.clone(),
                is_dir: e.is_dir,
                size: e.size,
                modified: e.modified,
            })
            .collect();
        for item in items.iter_mut().filter(|i| i.is_dir) {
            item.size = self
                .index
                .entries
                .iter()
                .filter(|e| !e.is_dir && in_item(&e.path, &item.name))
                .map(|e| e.size)
                .sum();
        }
        items.sort_by_key(|i| i.name.to_lowercase());
        Ok(items)
    }

    fn add(&mut self, source: &Path, dispose: Dispose) -> Result<String> {
        let key = self.key()?;
        let meta = fs::symlink_metadata(source).map_err(VaultError::io("lecture du fichier"))?;
        if meta.file_type().is_symlink() {
            return Err(VaultError::Symlink);
        }
        let base = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| VaultError::BadName(source.display().to_string()))?;
        let name = fsx::unique_name(&base, meta.is_dir(), |n| self.top_taken(n));
        let mut created = Vec::new();
        match self.add_inner(&key, source, &meta, &name, &mut created) {
            Ok(index) => self.index = index,
            Err(e) => {
                self.delete_objects(&created);
                return Err(e);
            }
        }
        // Tout est chiffré, vérifié et indexé : l'original peut partir à la corbeille.
        if let Err(e) = dispose(source) {
            // Annuler l'ajout : l'original est toujours là.
            let mut index = self.index.clone();
            index.entries.retain(|x| !in_item(&x.path, &name));
            match self.write_index(&key, &index) {
                Ok(()) => {
                    self.index = index;
                    self.delete_objects(&created);
                }
                Err(e2) => log::warn!("coffre : ajout non annulé ({e2}) ; l'original reste aussi"),
            }
            return Err(VaultError::Disposal(e.to_string()));
        }
        Ok(name)
    }

    fn extract(&mut self, name: &str, dest_dir: &Path) -> Result<PathBuf> {
        let key = self.key()?;
        if self.opened.contains_key(name) {
            // Les dernières modifications d'abord.
            self.sync_item(name)?;
        }
        let entries = self.item_entries(name)?;
        let is_dir = entries.first().is_some_and(|e| e.is_dir);
        let token = crypto::hex(&crypto::random::<6>()?);
        let staging = dest_dir.join(format!(".{name}.tidy-{token}"));
        let written = self
            .write_item(&key, name, &entries, &staging)
            .and_then(|()| {
                let target = fsx::unique_path(dest_dir, name, is_dir);
                fs::rename(&staging, &target)
                    .map(|()| target)
                    .map_err(VaultError::io("dépôt du fichier"))
            });
        let target = match written {
            Ok(t) => t,
            Err(e) => {
                // Copie partielle (créée par Tidy) : retirée ; le coffre n'a pas changé.
                if staging.is_dir() {
                    let _ = fs::remove_dir_all(&staging);
                } else {
                    let _ = fs::remove_file(&staging);
                }
                return Err(e);
            }
        };
        fsx::sync_dir(dest_dir);
        // Le fichier est en place : on peut le retirer du coffre.
        if let Err(e) = self.remove_item(&key, name) {
            log::warn!("coffre : élément rendu mais encore dans le coffre ({e})");
            return Ok(target);
        }
        let kept = self.forget_opened(name);
        if !kept.is_empty() {
            log::warn!(
                "coffre : {} copie(s) temporaire(s) non effacée(s)",
                kept.len()
            );
        }
        Ok(target)
    }

    fn open(&mut self, name: &str) -> Result<PathBuf> {
        let key = self.key()?;
        if let Some(o) = self.opened.get(name)
            && o.temp.join(&o.temp_name).exists()
        {
            return Ok(o.temp.join(&o.temp_name));
        }
        let entries = self.item_entries(name)?;
        let token = crypto::hex(&crypto::random::<8>()?);
        let temp = self.root.join(OPEN).join(token);
        let item = temp.join(name);
        fs::create_dir_all(&temp).map_err(VaultError::io("dossier temporaire"))?;
        if let Err(e) = self.write_item(&key, name, &entries, &item) {
            let _ = fsx::wipe_tree(&temp);
            return Err(e);
        }
        let mut states = HashMap::new();
        if let Ok(meta) = fs::metadata(&item) {
            if meta.is_dir() {
                for f in fsx::walk(&item).unwrap_or_default() {
                    if !f.is_dir {
                        states.insert(f.rel, FileState::of(&f.meta));
                    }
                }
            } else {
                states.insert(String::new(), FileState::of(&meta));
            }
        }
        self.opened.insert(
            name.to_owned(),
            Opened {
                temp,
                temp_name: name.to_owned(),
                states,
            },
        );
        Ok(item)
    }

    fn trash(&mut self, name: &str, dispose: Dispose) -> Result<()> {
        let key = self.key()?;
        if self.opened.contains_key(name) {
            self.sync_item(name)?;
        }
        let entries = self.item_entries(name)?;
        let token = crypto::hex(&crypto::random::<6>()?);
        let staging = self.root.join(STAGING).join(token);
        let item = staging.join(name);
        fs::create_dir_all(&staging).map_err(VaultError::io("dossier temporaire"))?;
        if let Err(e) = self.write_item(&key, name, &entries, &item) {
            let _ = fsx::wipe_tree(&staging);
            return Err(e);
        }
        if let Err(e) = dispose(&item) {
            let _ = fsx::wipe_tree(&staging);
            return Err(VaultError::Disposal(e.to_string()));
        }
        let _ = fs::remove_dir(&staging);
        if let Err(e) = self.remove_item(&key, name) {
            log::warn!("coffre : élément à la corbeille mais encore dans le coffre ({e})");
        }
        self.forget_opened(name);
        Ok(())
    }

    fn rename(&mut self, old: &str, new: &str) -> Result<String> {
        let key = self.key()?;
        if !fsx::valid_name(new) {
            return Err(VaultError::BadName(new.to_owned()));
        }
        self.item_entries(old)?;
        if new == old {
            return Ok(new.to_owned());
        }
        if new.to_lowercase() != old.to_lowercase() && self.top_taken(new) {
            return Err(VaultError::NameTaken(new.to_owned()));
        }
        let mut index = self.index.clone();
        for e in index.entries.iter_mut().filter(|e| in_item(&e.path, old)) {
            e.path = format!("{new}{}", &e.path[old.len()..]);
        }
        self.write_index(&key, &index)?;
        self.index = index;
        if let Some(o) = self.opened.remove(old) {
            self.opened.insert(new.to_owned(), o);
        }
        Ok(new.to_owned())
    }

    fn change_password(&mut self, old: &str, new: &str) -> Result<()> {
        check_password(new)?;
        let mut header = self.read_header()?;
        let key = Self::open_slot(&header.password, old.as_bytes(), &header.kdf, AAD_PASSWORD)?
            .ok_or(VaultError::WrongPassword)?;
        header.password = Self::make_slot(new.as_bytes(), &header.kdf, AAD_PASSWORD, &key)?;
        self.write_header(&header)
    }

    fn destroy(&mut self, password: &str, dest_dir: &Path) -> Result<Vec<PathBuf>> {
        let header = self.read_header()?;
        let key = Self::open_slot(
            &header.password,
            password.as_bytes(),
            &header.kdf,
            AAD_PASSWORD,
        )?
        .ok_or(VaultError::WrongPassword)?;
        if self.key.is_none() {
            self.finish_unlock(key)?;
        }
        let names: Vec<String> = self.top_names().map(str::to_owned).collect();
        let mut returned = Vec::new();
        for name in names {
            returned.push(self.extract(&name, dest_dir)?);
        }
        let report = self.lock();
        if !report.kept.is_empty() || self.has_leftovers() {
            // Des copies en clair restent (fichier encore ouvert) : on garde le dossier.
            return Err(VaultError::StillOpen);
        }
        // Le coffre est vide : seuls restent ses propres fichiers.
        let _ = fs::remove_dir_all(self.root.join(OBJECTS));
        let _ = fs::remove_dir_all(self.root.join(STAGING));
        let _ = fs::remove_dir(self.root.join(OPEN));
        let _ = fs::remove_file(self.root.join(HEADER));
        let _ = fs::remove_file(self.root.join(INDEX));
        let _ = fs::remove_dir(&self.root);
        Ok(returned)
    }
}

#[cfg(test)]
mod tests;
