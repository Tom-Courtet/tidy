//! Recherche floue dans l'index ([`search`]).
//!
//! `nucleo-matcher` compare la saisie au nom de chaque élément, sans tenir compte de
//! la casse ni des accents (« ete » trouve « Été »). Plusieurs mots : tous doivent
//! correspondre. Le classement ajoute au score de correspondance des bonus : nom exact,
//! élément ouvert récemment, application (onglet « Tout »). À égalité : nom le plus
//! court, puis le plus récent.
//!
//! Sur un gros index, les éléments sont répartis entre plusieurs fils.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
use tidy_core::category::{Category, classify_by_extension};

use crate::index::{DirId, Entry, Index};
use crate::tab::Tab;

/// Au-delà, la recherche se répartit entre plusieurs fils.
const PARALLEL_FROM: usize = 40_000;
/// Bonus : nom identique à la saisie (sans l'extension).
const BONUS_EXACT: u32 = 80;
/// Bonus : élément ouvert récemment depuis Tidy.
const BONUS_RECENT: u32 = 60;
/// Bonus : application (onglet « Tout »).
const BONUS_APP: u32 = 24;

/// Une recherche.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Query {
    pub text: String,
    pub tab: Tab,
    /// Nombre maximal de résultats.
    pub limit: usize,
    /// Éléments ouverts récemment, du plus récent au plus ancien (bonus de classement ;
    /// liste montrée quand la saisie est vide).
    pub recents: Vec<PathBuf>,
}

/// Un résultat.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    pub name: String,
    pub category: Category,
    pub is_dir: bool,
    /// Date de modification (secondes depuis 1970), 0 si inconnue.
    pub modified: i64,
    pub recent: bool,
    pub score: u32,
}

impl Hit {
    /// Nom affiché : sans `.lnk`, `.app` ni `.url` pour les applications.
    pub fn display_name(&self) -> &str {
        display_name(&self.name, self.category)
    }
}

/// Nom affiché d'un élément.
pub fn display_name(name: &str, category: Category) -> &str {
    if matches!(category, Category::Application | Category::Game) {
        for ext in [".lnk", ".app", ".url", ".appref-ms"] {
            // `get` : la coupure peut tomber au milieu d'un caractère (nom non ASCII).
            if let Some(cut) = name.len().checked_sub(ext.len()).filter(|&n| n > 0)
                && let (Some(stem), Some(tail)) = (name.get(..cut), name.get(cut..))
                && tail.eq_ignore_ascii_case(ext)
            {
                return stem;
            }
        }
    }
    name
}

/// Saisie sans accents ni majuscules, espaces superflus retirés.
pub fn normalize(text: &str) -> String {
    fold(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Texte sans accents ni majuscules.
pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .map(|c| nucleo_matcher::chars::to_lower_case(nucleo_matcher::chars::normalize(c)))
        .collect()
}

#[derive(Clone, Copy)]
struct Scored {
    score: u32,
    index: usize,
}

/// Résultats de `query` dans `index`. Saisie vide : les récents (si donnés), puis,
/// dans un onglet autre que « Tout », ses éléments (applications par nom, le reste du
/// plus récent au plus ancien).
pub fn search(index: &Index, query: &Query) -> Vec<Hit> {
    let limit = query.limit.max(1);
    let text = normalize(&query.text);
    if text.is_empty() {
        return browse(index, query, limit);
    }
    let mut pattern = Pattern::parse(&text, CaseMatching::Ignore, Normalization::Smart);
    // Le mot le plus long d'abord : il écarte le plus d'éléments, le plus tôt.
    pattern
        .atoms
        .sort_by_key(|a| std::cmp::Reverse(a.needle_text().len()));
    let recents: HashSet<(DirId, &str)> = query
        .recents
        .iter()
        .filter_map(|p| {
            let dir = index.dir_id(p.parent()?)?;
            Some((dir, p.file_name()?.to_str()?))
        })
        .collect();
    let ctx = ScoreCx {
        index,
        tab: query.tab,
        text: &text,
        recents: &recents,
        pattern: &pattern,
    };

    let entries = &index.entries;
    let mut scored: Vec<Scored> = if entries.len() < PARALLEL_FROM {
        ctx.score_range(0, entries, limit)
    } else {
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .clamp(2, 8);
        let chunk = entries.len().div_ceil(threads);
        std::thread::scope(|s| {
            let handles: Vec<_> = entries
                .chunks(chunk)
                .enumerate()
                .map(|(i, part)| {
                    let ctx = &ctx;
                    s.spawn(move || ctx.score_range(i * chunk, part, limit))
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect()
        })
    };
    keep_best(&mut scored, limit, entries);
    scored
        .into_iter()
        .map(|s| {
            let e = &entries[s.index];
            hit(index, e, s.score, recents.contains(&(e.dir, &*e.name)))
        })
        .collect()
}

struct ScoreCx<'a> {
    index: &'a Index,
    tab: Tab,
    text: &'a str,
    recents: &'a HashSet<(DirId, &'a str)>,
    pattern: &'a Pattern,
}

impl ScoreCx<'_> {
    /// Scores d'une tranche d'éléments (`base` : position de la tranche), bornés aux
    /// `limit` meilleurs.
    fn score_range(&self, base: usize, part: &[Entry], limit: usize) -> Vec<Scored> {
        let mut config = nucleo_matcher::Config::DEFAULT;
        config.prefer_prefix = true;
        let mut matcher = Matcher::new(config);
        let mut buf = Vec::new();
        let mut out = Vec::new();
        for (i, e) in part.iter().enumerate() {
            if self.tab != Tab::All && !self.tab.accepts(e.category, self.index.on_desktop(e)) {
                continue;
            }
            let Some(mut score) = self
                .pattern
                .score(Utf32Str::new(e.key(), &mut buf), &mut matcher)
            else {
                continue;
            };
            if is_exact(display_name(&e.name, e.category), self.text) {
                score += BONUS_EXACT;
            }
            if !self.recents.is_empty() && self.recents.contains(&(e.dir, &*e.name)) {
                score += BONUS_RECENT;
            }
            if self.tab == Tab::All && e.category == Category::Application {
                score += BONUS_APP;
            }
            out.push(Scored {
                score,
                index: base + i,
            });
            if out.len() >= limit * 8 {
                keep_best(&mut out, limit, &self.index.entries);
            }
        }
        out
    }
}

/// Nom (sans extension) identique à la saisie normalisée.
fn is_exact(name: &str, text: &str) -> bool {
    let stem = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    };
    stem.len() + 4 >= text.len() && text.len() + 4 >= stem.len() && normalize(stem) == text
}

/// Garde les `limit` meilleurs, triés.
fn keep_best(scored: &mut Vec<Scored>, limit: usize, entries: &[Entry]) {
    let order = |a: &Scored, b: &Scored| {
        let (ea, eb) = (&entries[a.index], &entries[b.index]);
        b.score
            .cmp(&a.score)
            .then(ea.name.len().cmp(&eb.name.len()))
            .then(eb.modified.cmp(&ea.modified))
            .then(ea.name.cmp(&eb.name))
    };
    if scored.len() > limit {
        scored.select_nth_unstable_by(limit, order);
        scored.truncate(limit);
    }
    scored.sort_unstable_by(order);
}

fn hit(index: &Index, e: &Entry, score: u32, recent: bool) -> Hit {
    Hit {
        path: index.path_of(e),
        name: e.name.to_string(),
        category: e.category,
        is_dir: e.is_dir,
        modified: e.modified,
        recent,
        score,
    }
}

/// Saisie vide : récents, puis les éléments de l'onglet.
fn browse(index: &Index, query: &Query, limit: usize) -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for path in &query.recents {
        if out.len() >= limit {
            break;
        }
        let Some(hit) = recent_hit(index, path) else {
            continue;
        };
        if query.tab.accepts(hit.category, on_desktop(index, path)) && seen.insert(path.clone()) {
            out.push(hit);
        }
    }
    if query.tab == Tab::All || out.len() >= limit {
        return out;
    }
    let mut picked: Vec<&Entry> = index
        .entries
        .iter()
        .filter(|e| query.tab.accepts(e.category, index.on_desktop(e)))
        .collect();
    if query.tab == Tab::Apps {
        picked.sort_unstable_by_key(|e| display_name(&e.name, e.category).to_lowercase());
    } else {
        picked.sort_unstable_by_key(|e| std::cmp::Reverse(e.modified));
    }
    for e in picked {
        if out.len() >= limit {
            break;
        }
        let path = index.path_of(e);
        if !seen.contains(&path) {
            out.push(hit(index, e, 0, false));
        }
    }
    out
}

fn on_desktop(index: &Index, path: &Path) -> bool {
    path.parent()
        .and_then(|p| index.dir_id(p))
        .is_some_and(|id| index.dirs[id as usize].flags & crate::index::FLAG_DESKTOP != 0)
}

/// Résultat pour un élément récent : depuis l'index s'il y est, sinon depuis le disque
/// (hors des dossiers indexés). `None` s'il n'existe plus.
fn recent_hit(index: &Index, path: &Path) -> Option<Hit> {
    let name = path.file_name()?.to_str()?;
    if let Some(dir) = path.parent().and_then(|p| index.dir_id(p))
        && let Some(e) = index
            .entries
            .iter()
            .find(|e| e.dir == dir && &*e.name == name)
    {
        return Some(hit(index, e, 0, true));
    }
    let meta = std::fs::metadata(path).ok()?;
    Some(Hit {
        path: path.to_path_buf(),
        name: name.to_owned(),
        category: classify_by_extension(name, meta.is_dir()),
        is_dir: meta.is_dir() && !crate::config::is_package(name),
        modified: 0,
        recent: true,
        score: 0,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::config::Root;

    #[test]
    fn display_name_never_cuts_inside_a_character() {
        // `.appref-ms` fait 10 octets : la coupure tomberait au milieu de « é ».
        assert_eq!(
            display_name("Pokémon 2.exe", Category::Application),
            "Pokémon 2.exe"
        );
        assert_eq!(
            display_name("Éditeur.LNK", Category::Application),
            "Éditeur"
        );
        assert_eq!(display_name(".lnk", Category::Application), ".lnk");
    }

    fn index() -> Index {
        let mut index = Index::new(vec![Root {
            path: "/bureau".into(),
            desktop: true,
            programs: false,
        }]);
        index.push_synthetic("/bureau", "Été 2024.jpg", false, 10);
        index.push_synthetic("/bureau", "Rapport annuel.pdf", false, 20);
        index.push_synthetic("/docs", "rapport.pdf", false, 30);
        index.push_synthetic("/docs", "Rapports", true, 40);
        index.push_synthetic("/apps", "Firefox.lnk", false, 5);
        index.push_synthetic("/docs", "film.mkv", false, 50);
        index.push_synthetic("/docs", "archive.zip", false, 60);
        index
    }

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.name.as_str()).collect()
    }

    fn query(text: &str, tab: Tab) -> Query {
        Query {
            text: text.into(),
            tab,
            limit: 50,
            recents: Vec::new(),
        }
    }

    #[test]
    fn ignores_case_and_accents() {
        let index = index();
        assert_eq!(
            names(&search(&index, &query("ete", Tab::All))),
            ["Été 2024.jpg"]
        );
        assert_eq!(
            names(&search(&index, &query("ÉTÉ", Tab::All))),
            ["Été 2024.jpg"]
        );
        assert_eq!(normalize("  Été   2024 "), "ete 2024");
    }

    #[test]
    fn ranks_exact_names_first_and_filters_tabs() {
        let index = index();
        let hits = search(&index, &query("rapport", Tab::All));
        assert_eq!(hits[0].name, "rapport.pdf", "nom exact d'abord");
        assert_eq!(hits.len(), 3);
        let folders = search(&index, &query("rapport", Tab::Folders));
        assert_eq!(names(&folders), ["Rapports"]);
        let desktop = search(&index, &query("rapport", Tab::Desktop));
        assert_eq!(names(&desktop), ["Rapport annuel.pdf"]);
        // Plusieurs mots : tous doivent correspondre.
        assert_eq!(
            names(&search(&index, &query("rap ann", Tab::All))),
            ["Rapport annuel.pdf"]
        );
        let apps = search(&index, &query("fire", Tab::Apps));
        assert_eq!(apps[0].display_name(), "Firefox");
    }

    #[test]
    fn recents_get_a_bonus_and_fill_an_empty_query() {
        let index = index();
        let mut q = query("rapp", Tab::All);
        q.recents = vec!["/bureau/Rapport annuel.pdf".into()];
        let hits = search(&index, &q);
        assert_eq!(hits[0].name, "Rapport annuel.pdf");
        assert!(hits[0].recent);
        q.text.clear();
        assert_eq!(names(&search(&index, &q)), ["Rapport annuel.pdf"]);
        // Onglet sans saisie : ses éléments, du plus récent au plus ancien.
        let docs = search(&index, &query("", Tab::Documents));
        assert_eq!(names(&docs), ["rapport.pdf", "Rapport annuel.pdf"]);
    }

    /// Nom de fichier plausible, tiré de `i`.
    fn synthetic_name(i: usize) -> String {
        const WORDS: [&str; 16] = [
            "rapport", "facture", "photo", "vacances", "projet", "budget", "réunion", "notes",
            "contrat", "devis", "scan", "image", "musique", "vidéo", "été", "client",
        ];
        const EXTS: [&str; 8] = ["pdf", "docx", "jpg", "png", "mp3", "mkv", "zip", "txt"];
        format!(
            "{} {} {} {i}.{}",
            WORDS[i % 16],
            WORDS[(i / 16) % 16],
            WORDS[(i / 256) % 16],
            EXTS[(i / 7) % 8]
        )
    }

    fn big_index(count: usize) -> Index {
        let mut index = Index::new(Vec::new());
        for i in 0..count {
            let dir = format!("/racine/dossier {}/sous {}", i % 500, i % 37);
            index.push_synthetic(&dir, &synthetic_name(i), false, i as i64);
        }
        index
    }

    fn timed(index: &Index, text: &str, tab: Tab) -> (std::time::Duration, usize) {
        let q = Query {
            text: text.into(),
            tab,
            limit: 200,
            recents: Vec::new(),
        };
        let start = Instant::now();
        let hits = search(index, &q);
        (start.elapsed(), hits.len())
    }

    /// Petit volume : résultats corrects et rapides, même en debug.
    #[test]
    fn small_index_is_fast() {
        let index = big_index(20_000);
        let (took, count) = timed(&index, "fact proj", Tab::All);
        assert!(count > 0);
        assert!(took.as_millis() < 500, "{took:?}");
    }

    /// Banc d'essai : 500 000 éléments, chaque requête en moins de 50 ms. À lancer à la
    /// main : `cargo test -p tidy-search --release -- --ignored --nocapture banc`.
    #[test]
    #[ignore]
    fn banc_500k() {
        let index = big_index(500_000);
        for (text, tab) in [
            ("r", Tab::All),
            ("rap", Tab::All),
            ("rapport facture", Tab::All),
            ("ete vac 1234", Tab::All),
            ("zzzz", Tab::All),
            ("photo", Tab::Images),
        ] {
            // Une requête à froid, puis la mesure.
            let _ = timed(&index, text, tab);
            let (took, count) = timed(&index, text, tab);
            println!("« {text} » ({tab:?}) : {count} résultats en {took:?}");
            assert!(took.as_millis() < 50, "« {text} » : {took:?}");
        }
    }
}
