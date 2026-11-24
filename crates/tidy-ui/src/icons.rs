//! Icônes de l'interface : glyphes de la police Lucide (ISC/MIT), réduite aux icônes
//! listées ici (`assets/fonts/Lucide.ttf`, régénérée par `tools/subset_icons.py`,
//! qui réécrit aussi ce fichier : ne pas le modifier à la main).
//!
//! Chaque constante est un texte d'un caractère, à mêler au texte normal :
//! `format!("{} Supprimer", icons::DELETE)`.

/// `package`
pub const BOX: &str = "\u{E129}";
/// `layout-dashboard`
pub const BOXES: &str = "\u{E1C1}";
/// `folder`
pub const FOLDER: &str = "\u{E0D7}";
/// `folder-open`
pub const FOLDER_OPEN: &str = "\u{E247}";
/// `folder-plus`
pub const FOLDER_PLUS: &str = "\u{E0D9}";
/// `folder-symlink`
pub const PORTAL: &str = "\u{E33B}";
/// `file`
pub const FILE: &str = "\u{E0C0}";
/// `file-text`
pub const DOCUMENT: &str = "\u{E0CC}";
/// `image`
pub const IMAGE: &str = "\u{E0F6}";
/// `film`
pub const VIDEO: &str = "\u{E0D0}";
/// `music`
pub const MUSIC: &str = "\u{E122}";
/// `file-archive`
pub const ARCHIVE: &str = "\u{E30D}";
/// `app-window`
pub const APP: &str = "\u{E426}";
/// `square-terminal`
pub const EXECUTABLE: &str = "\u{E20A}";
/// `link`
pub const LINK: &str = "\u{E102}";
/// `gamepad-2`
pub const GAME: &str = "\u{E0DF}";
/// `shapes`
pub const OTHER: &str = "\u{E4B3}";
/// `square-dashed-mouse-pointer`
pub const DRAW_BOX: &str = "\u{E509}";
/// `lock`
pub const LOCK: &str = "\u{E10B}";
/// `lock-open`
pub const UNLOCK: &str = "\u{E10C}";
/// `key-round`
pub const KEY: &str = "\u{E4A3}";
/// `shield-check`
pub const SHIELD: &str = "\u{E1FF}";
/// `house`
pub const HOME: &str = "\u{E0F5}";
/// `wallpaper`
pub const WALLPAPER: &str = "\u{E44B}";
/// `layout-grid`
pub const WIDGETS: &str = "\u{E0FF}";
/// `search`
pub const SEARCH: &str = "\u{E151}";
/// `settings`
pub const SETTINGS: &str = "\u{E154}";
/// `sliders-horizontal`
pub const OPTIONS: &str = "\u{E29A}";
/// `layers`
pub const LAYOUT: &str = "\u{E529}";
/// `history`
pub const SNAPSHOT: &str = "\u{E1F5}";
/// `bug`
pub const DIAGNOSTIC: &str = "\u{E20C}";
/// `mouse-pointer-click`
pub const QUICK_ACTIONS: &str = "\u{E120}";
/// `wand-sparkles`
pub const AUTO_TIDY: &str = "\u{E357}";
/// `rocket`
pub const ROCKET: &str = "\u{E286}";
/// `clock`
pub const CLOCK: &str = "\u{E087}";
/// `sticky-note`
pub const NOTE: &str = "\u{E303}";
/// `timer`
pub const TIMER: &str = "\u{E1E0}";
/// `cloud-sun`
pub const WEATHER: &str = "\u{E216}";
/// `calendar`
pub const CALENDAR: &str = "\u{E063}";
/// `sparkles`
pub const AI: &str = "\u{E412}";
/// `wrench`
pub const TOOLS: &str = "\u{E1B1}";
/// `camera`
pub const CAMERA: &str = "\u{E064}";
/// `bell`
pub const BELL: &str = "\u{E059}";
/// `plus`
pub const PLUS: &str = "\u{E13D}";
/// `x`
pub const CLOSE: &str = "\u{E1B2}";
/// `check`
pub const OK: &str = "\u{E06C}";
/// `trash-2`
pub const DELETE: &str = "\u{E18E}";
/// `pencil`
pub const EDIT: &str = "\u{E1F9}";
/// `copy`
pub const COPY: &str = "\u{E09E}";
/// `scissors`
pub const CUT: &str = "\u{E14E}";
/// `clipboard-paste`
pub const PASTE: &str = "\u{E3E8}";
/// `external-link`
pub const OPEN: &str = "\u{E0B9}";
/// `rotate-ccw`
pub const RESTORE: &str = "\u{E148}";
/// `undo-2`
pub const UNDO: &str = "\u{E2A1}";
/// `refresh-cw`
pub const REFRESH: &str = "\u{E145}";
/// `pin`
pub const PIN: &str = "\u{E259}";
/// `star`
pub const STAR: &str = "\u{E176}";
/// `eye`
pub const EYE: &str = "\u{E0BA}";
/// `eye-off`
pub const EYE_OFF: &str = "\u{E0BB}";
/// `power`
pub const POWER: &str = "\u{E140}";
/// `play`
pub const PLAY: &str = "\u{E13C}";
/// `pause`
pub const PAUSE: &str = "\u{E12E}";
/// `volume-2`
pub const VOLUME: &str = "\u{E1AB}";
/// `volume-x`
pub const MUTE: &str = "\u{E1AC}";
/// `menu`
pub const MENU: &str = "\u{E115}";
/// `arrow-up-down`
pub const SORT: &str = "\u{E37D}";
/// `chevrons-up`
pub const ROLL_UP: &str = "\u{E074}";
/// `chevrons-down`
pub const ROLL_DOWN: &str = "\u{E071}";
/// `panels-top-left`
pub const TABS: &str = "\u{E12C}";
/// `palette`
pub const PALETTE: &str = "\u{E1DD}";
/// `magnet`
pub const MAGNET: &str = "\u{E2B5}";
/// `grip-vertical`
pub const GRIP: &str = "\u{E0EB}";
/// `list`
pub const LIST: &str = "\u{E106}";
/// `grid-3x3`
pub const GRID: &str = "\u{E0E9}";
/// `monitor`
pub const MONITOR: &str = "\u{E11D}";
/// `keyboard`
pub const KEYBOARD: &str = "\u{E284}";
/// `languages`
pub const LANGUAGE: &str = "\u{E0FE}";
/// `globe`
pub const GLOBE: &str = "\u{E0E8}";
/// `info`
pub const INFO: &str = "\u{E0F9}";
/// `triangle-alert`
pub const WARNING: &str = "\u{E193}";
/// `chevron-down`
pub const CHEVRON_DOWN: &str = "\u{E06D}";
/// `chevron-up`
pub const CHEVRON_UP: &str = "\u{E070}";
/// `chevron-left`
pub const CHEVRON_LEFT: &str = "\u{E06E}";
/// `chevron-right`
pub const CHEVRON_RIGHT: &str = "\u{E06F}";
/// `arrow-left`
pub const BACK: &str = "\u{E048}";
/// `arrow-right`
pub const NEXT: &str = "\u{E049}";
/// `rows-3`
pub const ROWS: &str = "\u{E58A}";
/// `layout-list`
pub const LAYOUT_LIST: &str = "\u{E1D9}";
/// `arrow-down-a-z`
pub const SORT_NAME: &str = "\u{E415}";
/// `calendar-days`
pub const CALENDAR_DAYS: &str = "\u{E2B9}";
/// `hard-drive`
pub const DRIVE: &str = "\u{E0ED}";
/// `folder-tree`
pub const FOLDER_TREE: &str = "\u{E33C}";
/// `move`
pub const MOVE: &str = "\u{E121}";
/// `merge`
pub const MERGE: &str = "\u{E43F}";
/// `split`
pub const SPLIT: &str = "\u{E440}";
/// `type`
pub const TITLE: &str = "\u{E198}";
/// `square-round-corner`
pub const CORNERS: &str = "\u{E648}";
/// `blend`
pub const OPACITY: &str = "\u{E59C}";
/// `droplets`
pub const GLASS: &str = "\u{E0B5}";
/// `scaling`
pub const ICON_SIZE: &str = "\u{E2EC}";
/// `mouse-pointer-2`
pub const POINTER: &str = "\u{E1C3}";
/// `hand`
pub const HAND: &str = "\u{E1D7}";
/// `circle-check`
pub const DONE: &str = "\u{E226}";
/// `circle-alert`
pub const ALERT: &str = "\u{E077}";
/// `clipboard-copy`
pub const CLIPBOARD: &str = "\u{E225}";
/// `tag`
pub const TAG: &str = "\u{E17F}";
/// `inbox`
pub const INBOX: &str = "\u{E0F7}";
/// `zap`
pub const ZAP: &str = "\u{E1B4}";
/// `funnel`
pub const FILTER: &str = "\u{E0DC}";
/// `toggle-right`
pub const TOGGLE: &str = "\u{E18C}";
/// `paint-bucket`
pub const PAINT: &str = "\u{E2E6}";
/// `text-cursor-input`
pub const RENAME: &str = "\u{E265}";
/// `log-in`
pub const LOGIN: &str = "\u{E10D}";

/// (nom, glyphe) de toutes les icônes, pour les tests.
pub const ALL: &[(&str, &str)] = &[
    ("BOX", BOX),
    ("BOXES", BOXES),
    ("FOLDER", FOLDER),
    ("FOLDER_OPEN", FOLDER_OPEN),
    ("FOLDER_PLUS", FOLDER_PLUS),
    ("PORTAL", PORTAL),
    ("FILE", FILE),
    ("DOCUMENT", DOCUMENT),
    ("IMAGE", IMAGE),
    ("VIDEO", VIDEO),
    ("MUSIC", MUSIC),
    ("ARCHIVE", ARCHIVE),
    ("APP", APP),
    ("EXECUTABLE", EXECUTABLE),
    ("LINK", LINK),
    ("GAME", GAME),
    ("OTHER", OTHER),
    ("DRAW_BOX", DRAW_BOX),
    ("LOCK", LOCK),
    ("UNLOCK", UNLOCK),
    ("KEY", KEY),
    ("SHIELD", SHIELD),
    ("HOME", HOME),
    ("WALLPAPER", WALLPAPER),
    ("WIDGETS", WIDGETS),
    ("SEARCH", SEARCH),
    ("SETTINGS", SETTINGS),
    ("OPTIONS", OPTIONS),
    ("LAYOUT", LAYOUT),
    ("SNAPSHOT", SNAPSHOT),
    ("DIAGNOSTIC", DIAGNOSTIC),
    ("QUICK_ACTIONS", QUICK_ACTIONS),
    ("AUTO_TIDY", AUTO_TIDY),
    ("ROCKET", ROCKET),
    ("CLOCK", CLOCK),
    ("NOTE", NOTE),
    ("TIMER", TIMER),
    ("WEATHER", WEATHER),
    ("CALENDAR", CALENDAR),
    ("AI", AI),
    ("TOOLS", TOOLS),
    ("CAMERA", CAMERA),
    ("BELL", BELL),
    ("PLUS", PLUS),
    ("CLOSE", CLOSE),
    ("OK", OK),
    ("DELETE", DELETE),
    ("EDIT", EDIT),
    ("COPY", COPY),
    ("CUT", CUT),
    ("PASTE", PASTE),
    ("OPEN", OPEN),
    ("RESTORE", RESTORE),
    ("UNDO", UNDO),
    ("REFRESH", REFRESH),
    ("PIN", PIN),
    ("STAR", STAR),
    ("EYE", EYE),
    ("EYE_OFF", EYE_OFF),
    ("POWER", POWER),
    ("PLAY", PLAY),
    ("PAUSE", PAUSE),
    ("VOLUME", VOLUME),
    ("MUTE", MUTE),
    ("MENU", MENU),
    ("SORT", SORT),
    ("ROLL_UP", ROLL_UP),
    ("ROLL_DOWN", ROLL_DOWN),
    ("TABS", TABS),
    ("PALETTE", PALETTE),
    ("MAGNET", MAGNET),
    ("GRIP", GRIP),
    ("LIST", LIST),
    ("GRID", GRID),
    ("MONITOR", MONITOR),
    ("KEYBOARD", KEYBOARD),
    ("LANGUAGE", LANGUAGE),
    ("GLOBE", GLOBE),
    ("INFO", INFO),
    ("WARNING", WARNING),
    ("CHEVRON_DOWN", CHEVRON_DOWN),
    ("CHEVRON_UP", CHEVRON_UP),
    ("CHEVRON_LEFT", CHEVRON_LEFT),
    ("CHEVRON_RIGHT", CHEVRON_RIGHT),
    ("BACK", BACK),
    ("NEXT", NEXT),
    ("ROWS", ROWS),
    ("LAYOUT_LIST", LAYOUT_LIST),
    ("SORT_NAME", SORT_NAME),
    ("CALENDAR_DAYS", CALENDAR_DAYS),
    ("DRIVE", DRIVE),
    ("FOLDER_TREE", FOLDER_TREE),
    ("MOVE", MOVE),
    ("MERGE", MERGE),
    ("SPLIT", SPLIT),
    ("TITLE", TITLE),
    ("CORNERS", CORNERS),
    ("OPACITY", OPACITY),
    ("GLASS", GLASS),
    ("ICON_SIZE", ICON_SIZE),
    ("POINTER", POINTER),
    ("HAND", HAND),
    ("DONE", DONE),
    ("ALERT", ALERT),
    ("CLIPBOARD", CLIPBOARD),
    ("TAG", TAG),
    ("INBOX", INBOX),
    ("ZAP", ZAP),
    ("FILTER", FILTER),
    ("TOGGLE", TOGGLE),
    ("PAINT", PAINT),
    ("RENAME", RENAME),
    ("LOGIN", LOGIN),
];
