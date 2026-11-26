//! Tests du coffre, sur un faux bureau et une fausse corbeille (dossiers temporaires).

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::crypto::CHUNK;

const PW: &str = "secret12";

struct Bench {
    _dir: tempfile::TempDir,
    root: PathBuf,
    desktop: PathBuf,
    trash: PathBuf,
}

impl Bench {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("vault");
        let desktop = dir.path().join("Bureau");
        let trash = dir.path().join("Corbeille");
        fs::create_dir_all(&desktop).unwrap();
        fs::create_dir_all(&trash).unwrap();
        Self {
            _dir: dir,
            root,
            desktop,
            trash,
        }
    }

    fn vault(&self) -> FileVault {
        FileVault::with_params(
            &self.root,
            KdfParams {
                m_kib: 64,
                t: 1,
                p: 1,
            },
        )
    }

    /// Coffre créé et déverrouillé, avec sa clé de récupération.
    fn created(&self) -> (FileVault, RecoveryKey) {
        let mut v = self.vault();
        let key = RecoveryKey::generate().unwrap();
        v.create(PW, &key).unwrap();
        (v, key)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.desktop.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    /// « Corbeille » : déplace l'élément dans le dossier `Corbeille`.
    fn dispose(&self) -> impl FnMut(&Path) -> std::io::Result<()> + '_ {
        move |p: &Path| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            fs::rename(p, fsx::unique_path(&self.trash, &name, p.is_dir()))
        }
    }

    fn objects(&self) -> Vec<PathBuf> {
        fs::read_dir(self.root.join(OBJECTS))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }

    fn names(v: &FileVault) -> Vec<String> {
        v.list().unwrap().into_iter().map(|i| i.name).collect()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn roundtrip_through_lock_and_unlock() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let src = b.file("Relevé bancaire.pdf", b"contenu tres prive");
    let name = v.add(&src, &mut b.dispose()).unwrap();
    assert_eq!(name, "Relevé bancaire.pdf");
    // L'original est parti à la corbeille, pas supprimé.
    assert!(!src.exists());
    assert!(b.trash.join("Relevé bancaire.pdf").exists());
    v.lock();
    assert!(matches!(v.list(), Err(VaultError::Locked)));

    // Rien en clair dans le coffre : ni le contenu, ni le nom.
    for f in b.objects().into_iter().chain([b.root.join(INDEX)]) {
        let bytes = fs::read(f).unwrap();
        assert!(!contains(&bytes, b"tres prive"));
        assert!(!contains(&bytes, "Relevé".as_bytes()));
    }

    let mut v = b.vault();
    assert!(v.exists());
    v.unlock(PW).unwrap();
    let items = v.list().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].size, 18);
    assert!(!items[0].is_dir);
    let out = v.extract(&name, &b.desktop).unwrap();
    assert_eq!(out, b.desktop.join("Relevé bancaire.pdf"));
    assert_eq!(fs::read(&out).unwrap(), b"contenu tres prive");
    assert!(v.list().unwrap().is_empty());
    assert!(b.objects().is_empty());
}

#[test]
fn wrong_password_and_bad_lengths() {
    let b = Bench::new();
    let mut v = b.vault();
    let key = RecoveryKey::generate().unwrap();
    assert!(matches!(v.unlock(PW), Err(VaultError::NotCreated)));
    assert!(matches!(
        v.create("court", &key),
        Err(VaultError::BadPassword)
    ));
    assert!(matches!(
        v.create(&"x".repeat(21), &key),
        Err(VaultError::BadPassword)
    ));
    // 6 et 20 caractères (accents compris) : acceptés.
    assert!(check_password("éèàùçô").is_ok());
    assert!(check_password(&"é".repeat(20)).is_ok());
    v.create(PW, &key).unwrap();
    assert!(matches!(
        b.vault().create(PW, &key),
        Err(VaultError::AlreadyExists)
    ));
    v.lock();
    assert!(matches!(
        v.unlock("mauvais1"),
        Err(VaultError::WrongPassword)
    ));
    assert!(!v.is_unlocked());
    v.unlock(PW).unwrap();
    assert!(v.is_unlocked());
}

#[test]
fn recovery_key_resets_the_password() {
    let b = Bench::new();
    let (mut v, key) = b.created();
    let src = b.file("note.txt", b"abc");
    v.add(&src, &mut b.dispose()).unwrap();
    v.lock();
    let wrong = RecoveryKey::generate().unwrap();
    assert!(matches!(
        v.recover(&wrong.display(), "nouveau1"),
        Err(VaultError::WrongRecoveryKey)
    ));
    assert!(matches!(
        v.recover("pas une clé", "nouveau1"),
        Err(VaultError::BadRecoveryKey)
    ));
    // Saisie en minuscules, avec espaces.
    let typed = key.display().to_lowercase().replace('-', " ");
    v.recover(&typed, "nouveau1").unwrap();
    assert_eq!(Bench::names(&v), ["note.txt"]);
    v.lock();
    assert!(matches!(v.unlock(PW), Err(VaultError::WrongPassword)));
    v.unlock("nouveau1").unwrap();
    // La clé de récupération reste valable.
    v.lock();
    v.recover(&key.display(), "encore12").unwrap();
}

#[test]
fn change_password() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    assert!(matches!(
        v.change_password("mauvais1", "nouveau1"),
        Err(VaultError::WrongPassword)
    ));
    assert!(matches!(
        v.change_password(PW, "123"),
        Err(VaultError::BadPassword)
    ));
    v.change_password(PW, "nouveau1").unwrap();
    v.lock();
    assert!(v.unlock(PW).is_err());
    v.unlock("nouveau1").unwrap();
}

#[test]
fn big_files_take_several_chunks() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    for (name, len) in [("gros.bin", 2 * CHUNK + CHUNK / 2), ("pile.bin", 2 * CHUNK)] {
        let data: Vec<u8> = (0..len).map(|i| (i % 253) as u8).collect();
        let src = b.file(name, &data);
        v.add(&src, &mut b.dispose()).unwrap();
        let out = v.extract(name, &b.desktop).unwrap();
        assert_eq!(fs::read(out).unwrap(), data);
    }
}

#[test]
fn altered_file_is_detected_and_kept() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let src = b.file("a.txt", &vec![7u8; 5000]);
    v.add(&src, &mut b.dispose()).unwrap();
    let object = b.objects().pop().unwrap();
    let mut bytes = fs::read(&object).unwrap();
    bytes[100] ^= 0x40;
    fs::write(&object, bytes).unwrap();
    assert!(matches!(
        v.extract("a.txt", &b.desktop),
        Err(VaultError::Corrupt(_))
    ));
    // Rien d'écrit sur le bureau, l'élément reste dans le coffre.
    assert_eq!(fs::read_dir(&b.desktop).unwrap().count(), 0);
    assert_eq!(Bench::names(&v), ["a.txt"]);
    assert!(v.open("a.txt").is_err());
}

#[test]
fn altered_index_is_detected() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let src = b.file("a.txt", b"x");
    v.add(&src, &mut b.dispose()).unwrap();
    v.lock();
    let objects = b.objects();
    let index = b.root.join(INDEX);
    let mut bytes = fs::read(&index).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&index, &bytes).unwrap();
    assert!(matches!(v.unlock(PW), Err(VaultError::Corrupt(_))));
    assert!(!v.is_unlocked());
    // Les fichiers chiffrés ne sont pas pris pour des orphelins.
    assert_eq!(b.objects(), objects);
    fs::remove_file(&index).unwrap();
    assert!(matches!(v.unlock(PW), Err(VaultError::Corrupt(_))));
    assert_eq!(b.objects(), objects);
}

#[test]
fn interrupted_add_keeps_the_original() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let src = b.file("contrat.docx", b"version signee");

    // Panne entre les objets et l'index.
    v.fault = Some(Fault::BeforeIndex);
    assert!(v.add(&src, &mut b.dispose()).is_err());
    assert_eq!(fs::read(&src).unwrap(), b"version signee");
    assert!(Bench::names(&v).is_empty());
    assert!(b.objects().is_empty());

    // Objet abîmé avant la vérification.
    v.fault = Some(Fault::CorruptObject);
    assert!(matches!(
        v.add(&src, &mut b.dispose()),
        Err(VaultError::Verification(_))
    ));
    assert!(src.exists());
    assert!(b.objects().is_empty());

    // Coupure brutale (processus tué) après l'écriture d'un objet : l'objet orphelin
    // est supprimé au déverrouillage suivant, l'original n'a pas bougé.
    v.fault = None;
    let mut created = Vec::new();
    v.store_file(&v.key().unwrap(), &src, &mut created).unwrap();
    assert_eq!(b.objects().len(), 1);
    drop(v);
    let mut v = b.vault();
    v.unlock(PW).unwrap();
    assert!(b.objects().is_empty());
    assert!(src.exists());

    // La corbeille refuse : l'ajout est annulé.
    let mut refuse = |_: &Path| Err(std::io::Error::other("corbeille pleine"));
    assert!(matches!(
        v.add(&src, &mut refuse),
        Err(VaultError::Disposal(_))
    ));
    assert!(src.exists());
    assert!(Bench::names(&v).is_empty());
    assert!(b.objects().is_empty());

    // Enfin, normalement.
    v.add(&src, &mut b.dispose()).unwrap();
    assert!(!src.exists());
}

#[test]
fn folders_and_name_collisions() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let dir = b.desktop.join("Projets");
    fs::create_dir_all(dir.join("vide")).unwrap();
    fs::create_dir_all(dir.join("a/b")).unwrap();
    fs::write(dir.join("a/b/c.txt"), b"profond").unwrap();
    fs::write(dir.join("racine.txt"), b"12345").unwrap();
    assert_eq!(v.add(&dir, &mut b.dispose()).unwrap(), "Projets");
    let items = v.list().unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].is_dir);
    assert_eq!(items[0].size, 12);

    // Même nom : renommé dans le coffre.
    let again = b.file("Projets", b"un fichier");
    assert_eq!(v.add(&again, &mut b.dispose()).unwrap(), "Projets (2)");
    let other = b.file("x.txt", b"x");
    v.add(&other, &mut b.dispose()).unwrap();
    let other = b.file("x.txt", b"y");
    assert_eq!(v.add(&other, &mut b.dispose()).unwrap(), "x (2).txt");

    // Un fichier du même nom sur le bureau : l'élément rendu prend un autre nom.
    fs::create_dir_all(b.desktop.join("Projets")).unwrap();
    let out = v.extract("Projets", &b.desktop).unwrap();
    assert_eq!(out, b.desktop.join("Projets (2)"));
    assert_eq!(fs::read(out.join("a/b/c.txt")).unwrap(), b"profond");
    assert_eq!(fs::read(out.join("racine.txt")).unwrap(), b"12345");
    assert!(out.join("vide").is_dir());
    assert_eq!(Bench::names(&v), ["Projets (2)", "x (2).txt", "x.txt"]);
}

#[test]
fn rename_items() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let dir = b.desktop.join("D");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("f.txt"), b"f").unwrap();
    v.add(&dir, &mut b.dispose()).unwrap();
    let f = b.file("g.txt", b"g");
    v.add(&f, &mut b.dispose()).unwrap();
    assert!(matches!(
        v.rename("D", "g.txt"),
        Err(VaultError::NameTaken(_))
    ));
    assert!(matches!(v.rename("D", "a/b"), Err(VaultError::BadName(_))));
    assert!(matches!(v.rename("Z", "Y"), Err(VaultError::NotFound(_))));
    v.rename("D", "Dossier").unwrap();
    v.lock();
    v.unlock(PW).unwrap();
    assert_eq!(Bench::names(&v), ["Dossier", "g.txt"]);
    let out = v.extract("Dossier", &b.desktop).unwrap();
    assert_eq!(fs::read(out.join("f.txt")).unwrap(), b"f");
}

#[test]
fn opened_files_are_reencrypted_then_wiped() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let src = b.file("journal.txt", b"lundi");
    v.add(&src, &mut b.dispose()).unwrap();
    let dir = b.desktop.join("Album");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("1.txt"), b"un").unwrap();
    v.add(&dir, &mut b.dispose()).unwrap();

    let opened = v.open("journal.txt").unwrap();
    assert_eq!(fs::read(&opened).unwrap(), b"lundi");
    assert!(opened.starts_with(b.root.join(OPEN)));
    // Ouvert deux fois : même copie.
    assert_eq!(v.open("journal.txt").unwrap(), opened);
    fs::write(&opened, b"lundi et mardi").unwrap();
    let album = v.open("Album").unwrap();
    fs::write(album.join("2.txt"), b"deux").unwrap();

    let report = v.lock();
    assert_eq!(report.updated, 2);
    assert!(report.kept.is_empty());
    assert!(!opened.exists());
    assert!(!v.has_leftovers());

    v.unlock(PW).unwrap();
    let out = v.extract("journal.txt", &b.desktop).unwrap();
    assert_eq!(fs::read(out).unwrap(), b"lundi et mardi");
    let out = v.extract("Album", &b.desktop).unwrap();
    assert_eq!(fs::read(out.join("2.txt")).unwrap(), b"deux");
    // Anciennes versions effacées : plus aucun objet.
    assert!(b.objects().is_empty());
}

#[test]
fn leftovers_of_a_crash_are_recovered() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    let src = b.file("a.txt", b"avant");
    v.add(&src, &mut b.dispose()).unwrap();
    let src = b.file("b.txt", "inchangé".as_bytes());
    v.add(&src, &mut b.dispose()).unwrap();
    let a = v.open("a.txt").unwrap();
    v.open("b.txt").unwrap();
    fs::write(&a, b"apres").unwrap();
    // Plantage : ni verrouillage ni effacement.
    drop(v);
    let mut v = b.vault();
    assert!(v.has_leftovers());
    v.unlock(PW).unwrap();
    assert!(!v.has_leftovers());
    assert!(!a.exists());
    let out = v.extract("a.txt", &b.desktop).unwrap();
    assert_eq!(fs::read(out).unwrap(), b"apres");
    let out = v.extract("b.txt", &b.desktop).unwrap();
    assert_eq!(fs::read(out).unwrap(), "inchangé".as_bytes());
}

#[test]
fn trash_and_destroy_return_everything() {
    let b = Bench::new();
    let (mut v, _) = b.created();
    for n in ["1.txt", "2.txt", "3.txt"] {
        let src = b.file(n, n.as_bytes());
        v.add(&src, &mut b.dispose()).unwrap();
    }
    fs::remove_dir_all(&b.trash).unwrap();
    fs::create_dir_all(&b.trash).unwrap();
    // Corbeille d'un élément du coffre : en clair dans la corbeille.
    v.trash("1.txt", &mut b.dispose()).unwrap();
    assert_eq!(fs::read(b.trash.join("1.txt")).unwrap(), b"1.txt");
    let mut refuse = |_: &Path| Err(std::io::Error::other("non"));
    assert!(v.trash("2.txt", &mut refuse).is_err());
    assert_eq!(Bench::names(&v), ["2.txt", "3.txt"]);

    v.open("3.txt").unwrap();
    v.lock();
    assert!(matches!(
        v.destroy("mauvais1", &b.desktop),
        Err(VaultError::WrongPassword)
    ));
    let returned = v.destroy(PW, &b.desktop).unwrap();
    assert_eq!(returned.len(), 2);
    assert_eq!(fs::read(b.desktop.join("2.txt")).unwrap(), b"2.txt");
    assert_eq!(fs::read(b.desktop.join("3.txt")).unwrap(), b"3.txt");
    assert!(!b.root.exists());
    assert!(!v.exists());
}

#[test]
fn symlinks_are_refused() {
    #[cfg(unix)]
    {
        let b = Bench::new();
        let (mut v, _) = b.created();
        let target = b.file("cible.txt", b"x");
        let link = b.desktop.join("lien");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            v.add(&link, &mut b.dispose()),
            Err(VaultError::Symlink)
        ));
        assert!(link.exists());
        let dir = b.desktop.join("D");
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&target, dir.join("lien")).unwrap();
        assert!(v.add(&dir, &mut b.dispose()).is_err());
        assert!(dir.join("lien").exists());
        assert!(b.objects().is_empty());
    }
}
