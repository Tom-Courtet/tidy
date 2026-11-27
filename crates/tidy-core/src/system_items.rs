//! Éléments système du bureau de Windows (Corbeille, Ce PC, Réseau…) : ce ne sont pas
//! des fichiers du dossier Bureau, mais des objets du Shell désignés par leur nom
//! d'analyse `::{CLSID}`. Tidy les range dans les boîtes comme les fichiers, sous ce
//! nom (`ItemRef("::{…}")`), sans jamais les renommer ni les mettre à la corbeille.

/// Un élément système connu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemItem {
    /// Nom d'analyse du Shell, aussi nom de l'élément dans les boîtes.
    pub id: &'static str,
    /// Clé de traduction du libellé.
    pub label_key: &'static str,
}

/// Les éléments système que le bureau de Windows peut afficher (Paramètres > Thèmes >
/// Paramètres des icônes du bureau).
pub const ALL: [SystemItem; 5] = [
    SystemItem {
        id: "::{645FF040-5081-101B-9F08-08002B30309D}",
        label_key: "system_items.recycle_bin",
    },
    SystemItem {
        id: "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
        label_key: "system_items.this_pc",
    },
    SystemItem {
        id: "::{59031A47-3F72-44A7-89C5-5595FE6B30EE}",
        label_key: "system_items.user_folder",
    },
    SystemItem {
        id: "::{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}",
        label_key: "system_items.network",
    },
    SystemItem {
        id: "::{5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0}",
        label_key: "system_items.control_panel",
    },
];

/// Le nom désigne-t-il un élément système (et non un fichier) ?
pub fn is_system(name: &str) -> bool {
    name.starts_with("::{")
}

/// Élément système connu de ce nom (sans tenir compte de la casse du CLSID).
pub fn find(name: &str) -> Option<SystemItem> {
    ALL.iter()
        .copied()
        .find(|i| i.id.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_items_are_recognised() {
        assert!(is_system(ALL[0].id));
        assert!(!is_system("Rapport.pdf"));
        assert!(!is_system(":Rapport"));
        let lower = ALL[0].id.to_lowercase();
        assert_eq!(find(&lower), Some(ALL[0]));
        assert_eq!(find("::{00000000-0000-0000-0000-000000000000}"), None);
    }
}
