//! Ce que l'utilisateur demande depuis une boîte, un menu ou une fenêtre flottante.
//! Le dessin ne modifie rien : il rend des [`Action`], appliquées ensuite par
//! [`super::Desktop`].

use std::path::PathBuf;

use tidy_core::geometry::Rect;
use tidy_core::model::{BoxId, SortKey, ViewMode};

/// Une demande de l'utilisateur. Les éléments sont désignés par leur nom dans le dossier
/// de la boîte (le bureau, ou le dossier affiché par un portail).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // --- Éléments ---
    Open {
        b: BoxId,
        names: Vec<String>,
    },
    /// Menu d'éléments : natif si possible, sinon celui de Tidy.
    ItemMenu {
        b: BoxId,
        names: Vec<String>,
    },
    Reveal {
        b: BoxId,
        name: String,
    },
    /// Commencer à renommer (édition sous l'icône).
    StartRename {
        b: BoxId,
        name: String,
    },
    Rename {
        b: BoxId,
        old: String,
        new: String,
    },
    /// Demander confirmation avant la corbeille.
    AskTrash {
        b: BoxId,
        names: Vec<String>,
    },
    Trash {
        b: BoxId,
        names: Vec<String>,
    },
    CopyPath {
        b: BoxId,
        names: Vec<String>,
    },
    RemoveFromBox {
        b: BoxId,
        names: Vec<String>,
    },
    /// Fichiers déposés dans la boîte (glisser-déposer du système).
    DropFiles {
        b: BoxId,
        paths: Vec<PathBuf>,
        before: Option<String>,
    },
    /// Début d'un glisser d'éléments hors de la boîte.
    StartDrag {
        b: BoxId,
        names: Vec<String>,
    },
    /// Fin d'un glisser interne (repli sans glisser du système) ; `before` : élément de
    /// la boîte d'origine sous la souris.
    EndDrag {
        b: BoxId,
        before: Option<String>,
    },

    // --- Boîte ---
    BoxMenu {
        b: BoxId,
    },
    StartRenameBox {
        b: BoxId,
    },
    RenameBox {
        b: BoxId,
        name: String,
    },
    SetSort {
        b: BoxId,
        key: SortKey,
    },
    ToggleDescending {
        b: BoxId,
    },
    ToggleGroupByDate {
        b: BoxId,
    },
    SetView {
        b: BoxId,
        view: ViewMode,
    },
    SetIconSize {
        b: BoxId,
        size: f32,
    },
    ToggleRoll {
        b: BoxId,
    },
    ToggleAutoRoll {
        b: BoxId,
    },
    Appearance {
        b: BoxId,
    },
    NewPortal {
        near: BoxId,
    },
    /// Nouvelle boîte, à côté d'une autre, ou sur un cadre donné (dessin sur le bureau).
    NewBox {
        near: Option<BoxId>,
        rect: Option<Rect>,
    },
    NewTab {
        b: BoxId,
    },
    SelectTab {
        b: BoxId,
        index: usize,
    },
    RenameTab {
        b: BoxId,
        index: usize,
        name: String,
    },
    MergeWith {
        b: BoxId,
        other: BoxId,
    },
    DetachTab {
        b: BoxId,
        index: usize,
    },
    AskDeleteBox {
        b: BoxId,
    },
    DeleteBox {
        b: BoxId,
    },

    // --- Portails ---
    /// Entrer dans un sous-dossier du dossier affiché.
    PortalEnter {
        b: BoxId,
        name: String,
    },
    /// Revenir à un niveau du fil d'Ariane (0 : le dossier du portail).
    PortalUp {
        b: BoxId,
        depth: usize,
    },

    // --- Bandeau des non affectées ---
    ApplyRules,
    DismissUnassigned,
}
