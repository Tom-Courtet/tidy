//! Boîte privée (lot F) : verrouillage automatique, opacité…
//!
//! Fichier propre à son lot : les champs s'ajoutent ici, chacun avec une valeur par
//! défaut (`#[serde(default)]` sur la structure).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultSettings {}
