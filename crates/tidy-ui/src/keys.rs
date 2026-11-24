//! Libellés des raccourcis : symboles sous macOS (⇧⌘Z), noms en toutes lettres
//! ailleurs (Ctrl+Maj+Z). egui traduit déjà ⌘ en Ctrl pour les raccourcis eux-mêmes.

/// Raccourci avec la touche de commande (⌘ ou Ctrl), et ⇧/Maj si `shift`.
pub fn cmd(shift: bool, key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("{}⌘{key}", if shift { "⇧" } else { "" })
    } else {
        format!("Ctrl+{}{key}", if shift { "Maj+" } else { "" })
    }
}

/// Touche d'effacement.
pub const DELETE: &str = if cfg!(target_os = "macos") {
    "⌫"
} else {
    "Suppr"
};

/// « Afficher dans le Finder » / « dans l'Explorateur ».
pub const REVEAL: &str = if cfg!(target_os = "macos") {
    "Afficher dans le Finder"
} else {
    "Afficher dans l'Explorateur"
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_platform() {
        if cfg!(target_os = "macos") {
            assert_eq!(cmd(true, "Z"), "⇧⌘Z");
            assert_eq!(cmd(false, "C"), "⌘C");
        } else {
            assert_eq!(cmd(true, "Z"), "Ctrl+Maj+Z");
        }
    }
}
