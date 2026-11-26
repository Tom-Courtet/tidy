//! Contenu d'une boîte : quels éléments, dans quel ordre, et leur dessin en grille ou
//! en liste (icône système ou repli, libellé coupé proprement, sélection, ouverture,
//! renommage, glisser).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2, pos2, vec2,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use tidy_core::category::Category;
use tidy_core::grid::{self, Cell};
use tidy_core::model::{
    Appearance, BoxId, BoxKind, DeskBox, ItemRef, SortKey, SortOrder, Tab, ViewMode,
};
use tidy_core::rules::ItemInfo;
use tidy_core::sort::{DateGroup, date_group_in, sort_items};
use tidy_ui::{icons, t, theme};

use super::action::Action;
use super::textures::IconCache;

// ---------------------------------------------------------------------------
// Ordre et regroupement (logique pure)
// ---------------------------------------------------------------------------

/// Éléments d'un onglet présents sur le disque, dans l'ordre d'affichage. Un élément
/// rangé dont le fichier a disparu n'est pas montré (l'état n'est pas modifié).
pub fn tab_entries(tab: &Tab, files: &HashMap<String, ItemInfo>, sort: SortOrder) -> Vec<ItemInfo> {
    let mut items: Vec<ItemInfo> = tab
        .items
        .iter()
        .filter_map(|i| files.get(&i.0).cloned())
        .collect();
    sort_items(&mut items, sort);
    items
}

/// Contenu d'un dossier (portail). L'ordre « manuel » n'a pas de sens ici : dossiers
/// d'abord, puis par nom.
pub fn dir_entries(mut items: Vec<ItemInfo>, sort: SortOrder) -> Vec<ItemInfo> {
    if sort.key == SortKey::Manual {
        items.sort_by_key(|i| (!i.is_dir, i.name.to_lowercase()));
        if sort.descending {
            items.reverse();
        }
    } else {
        sort_items(&mut items, sort);
    }
    items
}

/// Cases notées de l'onglet actif quand la grille est libre : ordre manuel, sans
/// regroupement, vue en grille, boîte ordinaire. `None` : les éléments se suivent.
pub fn free_grid(b: &DeskBox) -> Option<&BTreeMap<ItemRef, Cell>> {
    let free = b.kind == BoxKind::Normal
        && b.view == ViewMode::Grid
        && b.sort.key == SortKey::Manual
        && !b.sort.group_by_date;
    if !free {
        return None;
    }
    b.tabs
        .get(b.active_tab.min(b.tabs.len().saturating_sub(1)))
        .map(|t| &t.cells)
}

/// Géométrie d'une grille libre affichée, pour trouver la case visée par un dépôt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridGeom {
    /// Coin haut gauche de la première case (coordonnées de la fenêtre, puis globales
    /// une fois rangée par le bureau).
    pub origin: Pos2,
    pub cell: Vec2,
    pub columns: usize,
}

impl GridGeom {
    /// Case sous le point `p` (même repère que `origin`), ramenée dans la largeur.
    pub fn cell_at(&self, p: Pos2) -> Cell {
        let col = ((p.x - self.origin.x) / self.cell.x).floor().max(0.0) as u32;
        let row = ((p.y - self.origin.y) / self.cell.y).floor().max(0.0) as u32;
        Cell::new(col.min(self.columns.saturating_sub(1) as u32), row)
    }

    /// Même grille, décalée de `by` (passage au repère global).
    pub fn translated(self, by: Vec2) -> Self {
        Self {
            origin: self.origin + by,
            ..self
        }
    }
}

/// Dossier des éléments d'une boîte : le bureau (avec le bureau public de Windows,
/// dont l'Explorateur mêle les éléments à ceux de l'utilisateur) ou le dossier affiché
/// par un portail.
#[derive(Debug, Clone)]
pub struct ItemDir {
    pub main: PathBuf,
    /// Bureau public et noms des éléments qui en viennent.
    pub public: Option<(PathBuf, Arc<HashSet<String>>)>,
}

impl ItemDir {
    pub fn plain(main: PathBuf) -> Self {
        Self { main, public: None }
    }

    /// Chemin de l'élément `name`.
    pub fn join(&self, name: &str) -> PathBuf {
        match &self.public {
            Some((dir, names)) if names.contains(name) => dir.join(name),
            _ => self.main.join(name),
        }
    }

    /// Dossier qui contient l'élément `name`.
    pub fn parent_of(&self, name: &str) -> &Path {
        match &self.public {
            Some((dir, names)) if names.contains(name) => dir,
            _ => &self.main,
        }
    }
}

/// Une section affichée : un groupe de dates (ou aucun) et ses éléments.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub group: Option<DateGroup>,
    pub items: Vec<ItemInfo>,
}

/// Découpe `items` (déjà triés) en sections. Sans regroupement : une seule section.
/// Avec : par groupe de dates ([`date_group_in`]), du plus récent au plus ancien,
/// l'ordre du tri gardé dans chacun (y compris l'ordre manuel).
pub fn sections(items: Vec<ItemInfo>, group: bool, now: Timestamp, tz: &TimeZone) -> Vec<Section> {
    if !group {
        return vec![Section { group: None, items }];
    }
    let mut out: Vec<Section> = Vec::new();
    let mut keyed: Vec<(DateGroup, ItemInfo)> = items
        .into_iter()
        .map(|i| (date_group_in(i.modified, now, tz), i))
        .collect();
    keyed.sort_by_key(|(g, _)| *g); // tri stable : l'ordre est gardé dans un groupe
    for (g, item) in keyed {
        match out.last_mut() {
            Some(s) if s.group == Some(g) => s.items.push(item),
            _ => out.push(Section {
                group: Some(g),
                items: vec![item],
            }),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Libellés, tailles, sélection (logique pure)
// ---------------------------------------------------------------------------

/// Coupe `name` à `max` caractères en gardant l'extension visible :
/// « Rapport annuel de l'association 2026.pdf » → « Rapport annuel d….pdf ».
pub fn ellipsize_middle(name: &str, max: usize) -> String {
    let count = name.chars().count();
    if count <= max || max < 4 {
        return name.to_owned();
    }
    let ext = match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= 7 => &name[dot..],
        _ => "",
    };
    let keep = max.saturating_sub(ext.chars().count() + 1).max(1);
    let stem: String = name.chars().take(keep).collect();
    format!("{}…{ext}", stem.trim_end())
}

/// Taille lisible (« 1,2 Mo »).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [u64; 3] = [1 << 30, 1 << 20, 1 << 10];
    let (value, key) = if bytes >= UNITS[0] {
        (bytes as f64 / UNITS[0] as f64, 3)
    } else if bytes >= UNITS[1] {
        (bytes as f64 / UNITS[1] as f64, 2)
    } else if bytes >= UNITS[2] {
        (bytes as f64 / UNITS[2] as f64, 1)
    } else {
        (bytes as f64, 0)
    };
    let n = if key == 0 || value >= 10.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}").replace('.', t!("box.decimal_separator"))
    };
    match key {
        0 => t!("box.size_b", n = n),
        1 => t!("box.size_kb", n = n),
        2 => t!("box.size_mb", n = n),
        _ => t!("box.size_gb", n = n),
    }
}

/// Façon de cliquer pour sélectionner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClickMods {
    /// Cmd (macOS) ou Ctrl : ajouter ou retirer.
    pub toggle: bool,
    /// Maj : étendre depuis l'ancre.
    pub range: bool,
}

/// Met à jour la sélection après un clic sur `name` ; `order` : ordre d'affichage.
pub fn click_select(
    selected: &mut BTreeSet<String>,
    anchor: &mut Option<String>,
    order: &[String],
    name: &str,
    mods: ClickMods,
) {
    if mods.range
        && let Some(a) = anchor.as_deref()
        && let (Some(i), Some(j)) = (
            order.iter().position(|n| n == a),
            order.iter().position(|n| n == name),
        )
    {
        let (lo, hi) = (i.min(j), i.max(j));
        if !mods.toggle {
            selected.clear();
        }
        selected.extend(order[lo..=hi].iter().cloned());
        return;
    }
    if mods.toggle {
        if !selected.remove(name) {
            selected.insert(name.to_owned());
        }
    } else {
        selected.clear();
        selected.insert(name.to_owned());
    }
    *anchor = Some(name.to_owned());
}

/// Sélection dans l'ordre d'affichage.
pub fn ordered_selection(selected: &BTreeSet<String>, order: &[String]) -> Vec<String> {
    order
        .iter()
        .filter(|n| selected.contains(*n))
        .cloned()
        .collect()
}

/// Mesures de la grille.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridMetrics {
    pub cell: Vec2,
    pub columns: usize,
}

/// Hauteur d'une ligne de libellé.
const LABEL_LINE: f32 = 15.0;

/// Grille pour une largeur et une taille d'icône : autant de colonnes que possible,
/// cellules élargies pour remplir la largeur.
pub fn grid_metrics(width: f32, icon: f32) -> GridMetrics {
    let min_w = (icon + 30.0).max(76.0);
    let columns = ((width / min_w).floor() as usize).max(1);
    let cell_w = (width / columns as f32).max(min_w.min(width));
    GridMetrics {
        cell: vec2(cell_w, icon + 14.0 + 2.0 * LABEL_LINE),
        columns,
    }
}

/// Hauteur d'une ligne de la liste.
const ROW_H: f32 = 28.0;
/// Hauteur d'un titre de section.
const SECTION_H: f32 = 24.0;

// ---------------------------------------------------------------------------
// Dessin
// ---------------------------------------------------------------------------

/// Édition du nom d'un élément.
#[derive(Debug, Clone, PartialEq)]
pub struct RenameEdit {
    pub old: String,
    pub text: String,
    /// Le champ doit prendre le focus (premier passage).
    pub focus: bool,
}

/// État d'interface du contenu d'une boîte.
#[derive(Debug, Default)]
pub struct ItemsState {
    pub selected: BTreeSet<String>,
    pub anchor: Option<String>,
    pub renaming: Option<RenameEdit>,
}

/// Ce qu'il faut pour dessiner le contenu.
pub struct ItemsView<'a> {
    pub b: BoxId,
    pub sections: &'a [Section],
    pub view: ViewMode,
    pub look: &'a Appearance,
    /// Dossier des éléments (bureau ou dossier du portail).
    pub dir: &'a ItemDir,
    pub single_click: bool,
    /// Le dossier affiché est celui d'un portail (double-clic sur un dossier : entrer).
    pub portal: bool,
    /// Grille libre : cases notées ([`free_grid`]).
    pub free: Option<&'a BTreeMap<ItemRef, Cell>>,
}

/// Résultat du dessin.
#[derive(Debug, Default)]
pub struct ItemsOutput {
    /// Élément sous la souris.
    pub hovered: Option<String>,
    /// Un élément est en train d'être glissé.
    pub dragging: bool,
    /// Grille libre affichée.
    pub grid: Option<GridGeom>,
}

/// Dessine le contenu dans `ui` (zone défilante) et rend les demandes dans `out`.
pub fn show(
    ui: &mut Ui,
    v: &ItemsView,
    st: &mut ItemsState,
    icons: &mut IconCache,
    out: &mut Vec<Action>,
) -> ItemsOutput {
    let order: Vec<String> = v
        .sections
        .iter()
        .flat_map(|s| s.items.iter().map(|i| i.name.clone()))
        .collect();
    st.selected.retain(|n| order.contains(n));
    let mut result = ItemsOutput::default();
    let visible_h = ui.available_height();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let width = ui.available_width();
            for section in v.sections {
                if let Some(g) = section.group {
                    section_title(ui, tidy_ui::i18n::tr(g.key()), width);
                }
                match v.view {
                    ViewMode::Grid => grid(
                        ui,
                        v,
                        section,
                        (width, visible_h),
                        &order,
                        st,
                        icons,
                        out,
                        &mut result,
                    ),
                    ViewMode::List => {
                        list(ui, v, section, width, &order, st, icons, out, &mut result)
                    }
                }
            }
        });
    keyboard(ui, v, st, &order, out);
    result
}

fn section_title(ui: &mut Ui, label: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, SECTION_H), Sense::hover());
    let painter = ui.painter();
    let text = painter.text(
        rect.left_center() + vec2(6.0, 2.0),
        Align2::LEFT_CENTER,
        label.to_uppercase(),
        theme::strong(theme::SIZE_XS),
        theme::TEXT_DIM,
    );
    let y = rect.center().y + 2.0;
    painter.line_segment(
        [pos2(text.right() + 8.0, y), pos2(rect.right() - 6.0, y)],
        Stroke::new(1.0, theme::BORDER),
    );
}

#[allow(clippy::too_many_arguments)]
fn grid(
    ui: &mut Ui,
    v: &ItemsView,
    section: &Section,
    (width, visible_h): (f32, f32),
    order: &[String],
    st: &mut ItemsState,
    icons: &mut IconCache,
    out: &mut Vec<Action>,
    result: &mut ItemsOutput,
) {
    let m = grid_metrics(width, v.look.icon_size);
    let names: Vec<&str> = section.items.iter().map(|i| i.name.as_str()).collect();
    let cells: Vec<Cell> = match v.free {
        Some(noted) => grid::layout(&names, noted, m.columns),
        None => (0..names.len())
            .map(|i| Cell::new((i % m.columns) as u32, (i / m.columns) as u32))
            .collect(),
    };
    let mut rows = grid::rows(&cells) as usize;
    if v.free.is_some() {
        // Une ligne vide de plus, et au moins la hauteur visible : on peut déposer sur
        // n'importe quelle case.
        rows = (rows + 1).max((visible_h / m.cell.y).floor() as usize);
    }
    let (area, _) = ui.allocate_exact_size(vec2(width, rows as f32 * m.cell.y), Sense::hover());
    let geom = GridGeom {
        origin: area.min,
        cell: m.cell,
        columns: m.columns,
    };
    for (info, at) in section.items.iter().zip(&cells) {
        let cell = Rect::from_min_size(
            area.min + vec2(at.col as f32 * m.cell.x, at.row as f32 * m.cell.y),
            m.cell,
        );
        if !ui.is_rect_visible(cell) {
            continue;
        }
        let icon = Rect::from_center_size(
            pos2(cell.center().x, cell.top() + 6.0 + v.look.icon_size / 2.0),
            Vec2::splat(v.look.icon_size),
        );
        let label = Rect::from_min_max(
            pos2(cell.left() + 4.0, icon.bottom() + 4.0),
            pos2(cell.right() - 4.0, cell.bottom() - 2.0),
        );
        item(
            ui, v, info, cell, icon, label, true, order, st, icons, out, result,
        );
    }
    if v.free.is_some() {
        result.grid = Some(geom);
        // Case visée pendant un glisser dans la boîte.
        if result.dragging
            && let Some(p) = ui.input(|i| i.pointer.hover_pos())
            && area.contains(p)
        {
            let at = geom.cell_at(p);
            let target = Rect::from_min_size(
                area.min + vec2(at.col as f32 * m.cell.x, at.row as f32 * m.cell.y),
                m.cell,
            )
            .shrink(3.0);
            ui.painter().rect_stroke(
                target,
                CornerRadius::same(10),
                Stroke::new(1.5, theme::ACCENT_2),
                StrokeKind::Inside,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn list(
    ui: &mut Ui,
    v: &ItemsView,
    section: &Section,
    width: f32,
    order: &[String],
    st: &mut ItemsState,
    icons: &mut IconCache,
    out: &mut Vec<Action>,
    result: &mut ItemsOutput,
) {
    let (area, _) = ui.allocate_exact_size(
        vec2(width, section.items.len() as f32 * ROW_H),
        Sense::hover(),
    );
    let icon_size = 20.0;
    for (i, info) in section.items.iter().enumerate() {
        let row = Rect::from_min_size(area.min + vec2(0.0, i as f32 * ROW_H), vec2(width, ROW_H));
        if !ui.is_rect_visible(row) {
            continue;
        }
        let icon = Rect::from_center_size(
            pos2(row.left() + 8.0 + icon_size / 2.0, row.center().y),
            Vec2::splat(icon_size),
        );
        let label = Rect::from_min_max(
            pos2(icon.right() + 8.0, row.top()),
            pos2(row.right() - 6.0, row.bottom()),
        );
        item(
            ui, v, info, row, icon, label, false, order, st, icons, out, result,
        );
    }
}

/// Un élément : fond, icône, libellé, interactions.
#[allow(clippy::too_many_arguments)]
fn item(
    ui: &mut Ui,
    v: &ItemsView,
    info: &ItemInfo,
    cell: Rect,
    icon: Rect,
    label: Rect,
    grid: bool,
    order: &[String],
    st: &mut ItemsState,
    icons: &mut IconCache,
    out: &mut Vec<Action>,
    result: &mut ItemsOutput,
) {
    let name = &info.name;
    let id = ui.scope_id().with(("élément", name));
    let resp = ui.interact(cell.shrink(2.0), id, Sense::click_and_drag());
    if resp.contains_pointer() {
        result.hovered = Some(name.clone());
    }
    let selected = st.selected.contains(name);
    let painter = ui.painter_at(cell);
    let bg = cell.shrink(2.0);
    let radius = CornerRadius::same(10);
    if selected {
        painter.rect_filled(bg, radius, theme::ACCENT.gamma_multiply(0.38));
        painter.rect_stroke(
            bg,
            radius,
            Stroke::new(1.0, theme::ACCENT),
            StrokeKind::Inside,
        );
    } else if resp.hovered() {
        painter.rect_filled(bg, radius, theme::TEXT.gamma_multiply(0.07));
    }

    paint_icon(ui, icon, info, &v.dir.join(name), icons);

    let renaming = st.renaming.as_ref().is_some_and(|r| &r.old == name);
    if renaming {
        rename_field(ui, v, label, grid, st, out);
    } else {
        paint_label(ui, v, info, label, grid);
    }

    // Clics.
    let mods = ui.input(|i| i.modifiers);
    let click_mods = ClickMods {
        toggle: mods.command,
        range: mods.shift,
    };
    if resp.clicked() {
        click_select(&mut st.selected, &mut st.anchor, order, name, click_mods);
        if v.single_click && !mods.any() {
            open(v, info, out);
        }
    }
    if resp.double_clicked() && !v.single_click {
        st.selected.clear();
        st.selected.insert(name.clone());
        open(v, info, out);
    }
    if resp.secondary_clicked() {
        if !selected {
            click_select(
                &mut st.selected,
                &mut st.anchor,
                order,
                name,
                ClickMods::default(),
            );
        }
        out.push(Action::ItemMenu {
            b: v.b,
            names: ordered_selection(&st.selected, order),
        });
    }
    // Glisser.
    if resp.drag_started() {
        if !st.selected.contains(name) {
            click_select(
                &mut st.selected,
                &mut st.anchor,
                order,
                name,
                ClickMods::default(),
            );
        }
        // L'élément saisi d'abord : c'est lui qui se pose sur la case visée.
        let mut names = ordered_selection(&st.selected, order);
        if let Some(i) = names.iter().position(|n| n == name) {
            let grabbed = names.remove(i);
            names.insert(0, grabbed);
        }
        out.push(Action::StartDrag { b: v.b, names });
    }
    if resp.dragged() {
        result.dragging = true;
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    if resp.drag_stopped() {
        out.push(Action::EndDrag {
            b: v.b,
            before: None,
        });
    }
    if resp.hovered() && !resp.dragged() {
        resp.on_hover_text_at_pointer(name);
    }
}

fn open(v: &ItemsView, info: &ItemInfo, out: &mut Vec<Action>) {
    if v.portal && info.is_dir {
        out.push(Action::PortalEnter {
            b: v.b,
            name: info.name.clone(),
        });
    } else {
        out.push(Action::Open {
            b: v.b,
            names: vec![info.name.clone()],
        });
    }
}

/// Icône Lucide et couleur du repli, par catégorie.
pub fn fallback_glyph(category: Category) -> (&'static str, Color32) {
    match category {
        Category::Application => (icons::APP, theme::ACCENT),
        Category::Folder => (icons::FOLDER, theme::ACCENT_2),
        Category::Document => (icons::DOCUMENT, theme::TEXT),
        Category::Image => (icons::IMAGE, theme::ACCENT),
        Category::Video => (icons::VIDEO, theme::ACCENT),
        Category::Music => (icons::MUSIC, theme::ACCENT),
        Category::Archive => (icons::ARCHIVE, theme::TEXT),
        Category::Executable => (icons::EXECUTABLE, theme::ACCENT),
        Category::WebLink => (icons::LINK, theme::TEXT),
        Category::Game => (icons::GAME, theme::ACCENT),
        Category::Other => (icons::FILE, theme::TEXT_DIM),
    }
}

fn paint_icon(ui: &Ui, rect: Rect, info: &ItemInfo, path: &Path, icons: &mut IconCache) {
    let px = (rect.width() * ui.ctx().pixels_per_point()).round() as u32;
    let thumbnail = matches!(info.category, Category::Image | Category::Video);
    let painter = ui.painter();
    if let Some(texture) = icons.get(path, px, thumbnail) {
        painter.image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }
    let (glyph, color) = fallback_glyph(info.category);
    let tile = rect.shrink(rect.width() * 0.06);
    let radius = CornerRadius::same((tile.width() * 0.24).clamp(4.0, 16.0) as u8);
    painter.rect_filled(tile, radius, theme::SURFACE_HI);
    painter.rect_stroke(
        tile,
        radius,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    painter.text(
        tile.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(tile.width() * 0.5),
        color,
    );
}

fn label_font(look: &Appearance) -> FontId {
    FontId::proportional(if look.icon_size >= 64.0 {
        theme::SIZE_M - 1.0
    } else {
        theme::SIZE_S
    })
}

fn paint_label(ui: &Ui, v: &ItemsView, info: &ItemInfo, rect: Rect, grid: bool) {
    let font = label_font(v.look);
    let color = super::boxwin::color(v.look.label_color, 1.0);
    // Estimation du nombre de caractères qui tiennent, pour garder l'extension.
    let per_line = (rect.width() / (font.size * 0.6)).floor().max(4.0) as usize;
    let lines = if grid { 2 } else { 1 };
    let text = ellipsize_middle(&info.name, per_line * lines);
    let mut job = LayoutJob::simple(text, font, color, rect.width());
    job.wrap = TextWrapping {
        max_width: rect.width(),
        max_rows: lines,
        // Grille : couper entre les mots ; liste : une ligne, coupée n'importe où.
        break_anywhere: !grid,
        overflow_character: Some('…'),
    };
    if grid {
        job.halign = egui::Align::Center;
    }
    let galley = ui.painter().layout_job(job);
    let pos = if grid {
        pos2(rect.center().x, rect.top())
    } else {
        pos2(rect.left(), rect.center().y - galley.size().y / 2.0)
    };
    let painter = ui.painter_at(rect.expand(2.0));
    if v.look.label_shadow {
        painter.galley_with_override_text_color(pos + vec2(0.0, 1.0), galley.clone(), theme::BG);
    }
    painter.galley(pos, galley, color);
    // Liste : taille à droite, s'il y a la place.
    if !grid && !info.is_dir && rect.width() > 240.0 {
        painter.text(
            rect.right_center(),
            Align2::RIGHT_CENTER,
            format_size(info.size),
            FontId::proportional(theme::SIZE_XS),
            theme::TEXT_DIM,
        );
    }
}

fn rename_field(
    ui: &mut Ui,
    v: &ItemsView,
    rect: Rect,
    grid: bool,
    st: &mut ItemsState,
    out: &mut Vec<Action>,
) {
    let Some(edit) = st.renaming.as_mut() else {
        return;
    };
    let field = if grid {
        Rect::from_min_size(rect.min, vec2(rect.width(), 22.0))
    } else {
        Rect::from_min_size(
            pos2(rect.left(), rect.center().y - 11.0),
            vec2(rect.width(), 22.0),
        )
    };
    let resp = ui.put(
        field,
        TextEdit::singleline(&mut edit.text)
            .font(label_font(v.look))
            .margin(vec2(4.0, 2.0))
            .align(if grid {
                Align2::CENTER_CENTER
            } else {
                Align2::LEFT_CENTER
            }),
    );
    if edit.focus {
        edit.focus = false;
        resp.request_focus();
    }
    if resp.lost_focus() {
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        let edit = st.renaming.take().unwrap_or_else(|| unreachable!());
        if !cancel && edit.text.trim() != edit.old {
            out.push(Action::Rename {
                b: v.b,
                old: edit.old,
                new: edit.text.trim().to_owned(),
            });
        }
    }
}

/// Raccourcis du contenu (fenêtre active, aucun champ en édition).
fn keyboard(ui: &Ui, v: &ItemsView, st: &mut ItemsState, order: &[String], out: &mut Vec<Action>) {
    let ctx = ui.ctx();
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
    if !focused || st.renaming.is_some() || ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let names = ordered_selection(&st.selected, order);
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::COMMAND, Key::A) {
            st.selected = order.iter().cloned().collect();
        }
        if names.is_empty() {
            return;
        }
        if i.consume_key(Modifiers::NONE, Key::Enter) {
            out.push(Action::Open {
                b: v.b,
                names: names.clone(),
            });
        } else if i.consume_key(Modifiers::NONE, Key::F2) && names.len() == 1 {
            out.push(Action::StartRename {
                b: v.b,
                name: names[0].clone(),
            });
        } else if i.consume_key(Modifiers::NONE, Key::Delete)
            || i.consume_key(Modifiers::COMMAND, Key::Backspace)
        {
            out.push(Action::AskTrash {
                b: v.b,
                names: names.clone(),
            });
        } else if i.consume_key(Modifiers::NONE, Key::Escape) {
            st.selected.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::model::ItemRef;

    fn info(name: &str, size: u64, modified: Option<&str>) -> ItemInfo {
        ItemInfo {
            name: name.into(),
            is_dir: !name.contains('.'),
            category: Category::Other,
            size,
            modified: modified.map(|m| m.parse::<Timestamp>().unwrap()),
            uti: None,
        }
    }

    fn names(items: &[ItemInfo]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn tab_keeps_manual_order_and_hides_missing_files() {
        let mut tab = Tab::new("");
        for n in ["c.txt", "absent.txt", "a.txt"] {
            tab.items.push(ItemRef(n.into()));
        }
        let files: HashMap<String, ItemInfo> = ["a.txt", "c.txt", "b.txt"]
            .iter()
            .map(|n| ((*n).to_owned(), info(n, 1, None)))
            .collect();
        let manual = tab_entries(&tab, &files, SortOrder::default());
        assert_eq!(names(&manual), ["c.txt", "a.txt"]);
        let by_name = tab_entries(
            &tab,
            &files,
            SortOrder {
                key: SortKey::Name,
                ..Default::default()
            },
        );
        assert_eq!(names(&by_name), ["a.txt", "c.txt"]);
    }

    #[test]
    fn portal_lists_folders_first() {
        let items = vec![
            info("b.txt", 1, None),
            info("Zeta", 0, None),
            info("a.txt", 1, None),
        ];
        assert_eq!(
            names(&dir_entries(items, SortOrder::default())),
            ["Zeta", "a.txt", "b.txt"]
        );
    }

    #[test]
    fn sections_keep_the_sort_inside_each_group() {
        let now: Timestamp = "2026-11-24T15:00:00Z".parse().unwrap();
        let items = vec![
            info("vieux.txt", 1, Some("2020-01-01T00:00:00Z")),
            info("b.txt", 1, Some("2026-11-24T10:00:00Z")),
            info("hier.txt", 1, Some("2026-11-23T10:00:00Z")),
            info("a.txt", 1, Some("2026-11-24T09:00:00Z")),
        ];
        let s = sections(items.clone(), true, now, &TimeZone::UTC);
        let groups: Vec<_> = s.iter().map(|s| s.group).collect();
        assert_eq!(
            groups,
            [
                Some(DateGroup::Today),
                Some(DateGroup::Yesterday),
                Some(DateGroup::Older)
            ]
        );
        assert_eq!(names(&s[0].items), ["b.txt", "a.txt"]);
        let flat = sections(items, false, now, &TimeZone::UTC);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].items.len(), 4);
    }

    #[test]
    fn long_names_keep_their_extension() {
        assert_eq!(ellipsize_middle("court.pdf", 20), "court.pdf");
        let long = "Rapport annuel de l'association 2026.pdf";
        let short = ellipsize_middle(long, 18);
        assert!(short.ends_with("….pdf"), "{short}");
        assert!(short.chars().count() <= 18, "{short}");
        // Pas d'extension (ou trop longue) : coupé à la fin.
        assert_eq!(ellipsize_middle("Un dossier très long", 10), "Un dossie…");
    }

    #[test]
    fn sizes_are_readable() {
        tidy_ui::set_language(tidy_ui::Lang::En);
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(25 * 1024 * 1024), "25 MB");
        tidy_ui::set_language(tidy_ui::Lang::Fr);
        assert_eq!(format_size(1536), "1,5 Ko");
    }

    #[test]
    fn selection_with_toggle_and_range() {
        let order: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| (*s).into()).collect();
        let mut sel = BTreeSet::new();
        let mut anchor = None;
        click_select(&mut sel, &mut anchor, &order, "b", ClickMods::default());
        click_select(
            &mut sel,
            &mut anchor,
            &order,
            "d",
            ClickMods {
                range: true,
                ..Default::default()
            },
        );
        assert_eq!(ordered_selection(&sel, &order), ["b", "c", "d"]);
        click_select(
            &mut sel,
            &mut anchor,
            &order,
            "c",
            ClickMods {
                toggle: true,
                ..Default::default()
            },
        );
        assert_eq!(ordered_selection(&sel, &order), ["b", "d"]);
        click_select(&mut sel, &mut anchor, &order, "a", ClickMods::default());
        assert_eq!(ordered_selection(&sel, &order), ["a"]);
    }

    #[test]
    fn grid_fills_the_width() {
        let m = grid_metrics(320.0, 48.0);
        assert_eq!(m.columns, 4);
        assert!((m.cell.x * 4.0 - 320.0).abs() < 0.01);
        // Très étroit : une colonne.
        assert_eq!(grid_metrics(50.0, 48.0).columns, 1);
    }
}
