//! Traductions : français (référence) et anglais.
//!
//! Les textes sont dans `locales/<langue>.toml`, embarqués dans l'exécutable. Une clé
//! s'écrit `section.clé` ; `{nom}` est remplacé par la valeur passée à [`t!`] :
//!
//! ```ignore
//! ui.label(t!("tray.quit"));
//! ui.label(t!("home.desktop_mode", mode = "géré"));
//! ```
//!
//! Une clé absente de la langue courante retombe sur le français, puis sur la clé
//! elle-même (visible, donc facile à repérer). Ajouter une langue : un fichier TOML,
//! une variante de [`Lang`] et une ligne dans [`table`].

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// Langue de l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    /// Nom de la langue, dans cette langue.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
        }
    }

    /// Langue d'après un code (`fr`, `fr-FR`, `en_US.UTF-8`…).
    pub fn from_code(code: &str) -> Option<Lang> {
        let lower = code.to_ascii_lowercase();
        if lower.starts_with("fr") {
            Some(Lang::Fr)
        } else if lower.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }
}

const FR: &str = include_str!("../locales/fr.toml");
const EN: &str = include_str!("../locales/en.toml");

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Change la langue de l'interface (prise en compte à l'image suivante).
pub fn set_language(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn language() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Fr,
    }
}

/// Table aplatie (`section.clé` → texte) d'une langue.
fn table(lang: Lang) -> &'static HashMap<String, String> {
    static FR_TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    static EN_TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    match lang {
        Lang::Fr => FR_TABLE.get_or_init(|| parse(FR)),
        Lang::En => EN_TABLE.get_or_init(|| parse(EN)),
    }
}

/// Aplatit un fichier TOML en `section.clé`. Un fichier invalide est une erreur de
/// développement : les tests le détectent, et en production la table reste vide.
fn parse(source: &str) -> HashMap<String, String> {
    fn walk(prefix: &str, value: &toml::Value, out: &mut HashMap<String, String>) {
        match value {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    let key = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&key, v, out);
                }
            }
            toml::Value::String(s) => {
                out.insert(prefix.to_owned(), s.clone());
            }
            other => {
                out.insert(prefix.to_owned(), other.to_string());
            }
        }
    }
    let mut out = HashMap::new();
    match source.parse::<toml::Table>() {
        Ok(t) => walk("", &toml::Value::Table(t), &mut out),
        Err(e) => log::error!("traductions illisibles : {e}"),
    }
    out
}

/// Texte de `key` dans la langue courante.
pub fn tr(key: &'static str) -> &'static str {
    lookup(language(), key)
}

fn lookup(lang: Lang, key: &'static str) -> &'static str {
    if let Some(text) = table(lang).get(key) {
        return text;
    }
    if let Some(text) = table(Lang::Fr).get(key) {
        return text;
    }
    log::debug!("traduction absente : {key}");
    key
}

/// Texte de `key` avec ses variables remplacées.
pub fn tr_args(key: &'static str, args: &[(&str, String)]) -> String {
    let mut text = tr(key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Texte traduit. `t!("clé")` donne un `&'static str` ; `t!("clé", nom = valeur)`
/// donne une `String` avec `{nom}` remplacé.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::tr_args($key, &[$((stringify!($name), $value.to_string())),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn every_language_has_the_same_keys() {
        let fr: BTreeSet<_> = table(Lang::Fr).keys().collect();
        assert!(!fr.is_empty(), "fr.toml illisible");
        for lang in Lang::ALL {
            let keys: BTreeSet<_> = table(lang).keys().collect();
            let missing: Vec<_> = fr.difference(&keys).collect();
            let extra: Vec<_> = keys.difference(&fr).collect();
            assert!(missing.is_empty(), "{lang:?} : clés absentes {missing:?}");
            assert!(extra.is_empty(), "{lang:?} : clés en trop {extra:?}");
        }
    }

    #[test]
    fn variables_are_replaced() {
        let text = tr_args("home.desktop_mode", &[("mode", "géré".into())]);
        assert!(text.contains("géré"), "{text}");
        assert!(!text.contains('{'));
    }

    #[test]
    fn unknown_key_falls_back_to_itself() {
        assert_eq!(lookup(Lang::En, "pas.une.clé"), "pas.une.clé");
    }

    #[test]
    fn language_codes() {
        assert_eq!(Lang::from_code("fr_FR.UTF-8"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("en-GB"), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
    }

    /// Toute clé utilisée par `t!("…")` dans le dépôt existe en français.
    #[test]
    fn every_used_key_exists() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut missing = Vec::new();
        let mut stack = vec![root.join("app/src"), root.join("crates")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if !path.ends_with("target") {
                        stack.push(path);
                    }
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap_or_default();
                    for (start, _) in source.match_indices("t!(\"") {
                        // `format!("…")` n'est pas `t!("…")`.
                        let before = source[..start].chars().next_back();
                        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        let rest = &source[start + 4..];
                        let key = rest.split('"').next().unwrap_or_default();
                        if key.contains('.')
                            && !key.contains(' ')
                            && !table(Lang::Fr).contains_key(key)
                        {
                            missing.push(format!("{} : {key}", path.display()));
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "clés absentes :\n{}", missing.join("\n"));
    }
}
