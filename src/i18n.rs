//! All user-facing text, in German and English. Kept short and friendly.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    pub fn detect() -> Self {
        match sys_locale::get_locale() {
            Some(l) if l.starts_with("de") => Lang::De,
            _ => Lang::En,
        }
    }

    pub fn t(self) -> &'static Strings {
        match self {
            Lang::De => &DE,
            Lang::En => &EN,
        }
    }
}

pub struct Strings {
    pub tab_search: &'static str,
    pub tab_favourites: &'static str,
    pub tab_library: &'static str,
    pub tab_settings: &'static str,
    pub search_hint: &'static str,
    pub search: &'static str,
    pub start_hint: &'static str,
    pub nothing_found: &'static str,
    pub search_failed: &'static str,
    pub minutes: &'static str,
    pub watch: &'static str,
    pub save: &'static str,
    pub saved: &'static str,
    pub saving: &'static str,
    pub no_favourites: &'static str,
    pub no_downloads: &'static str,
    pub bookmark: &'static str,
    pub unbookmark: &'static str,
    pub delete: &'static str,
    pub really_delete: &'static str,
    pub failed: &'static str,
    pub clear_finished: &'static str,
    pub close: &'static str,
    pub cannot_play: &'static str,
    pub open_other_app: &'static str,
    pub fullscreen: &'static str,
    pub language: &'static str,
    pub quality: &'static str,
    pub quality_high: &'static str,
    pub quality_medium: &'static str,
    pub quality_low: &'static str,
    pub quality_note: &'static str,
    pub save_location: &'static str,
    pub open_folder: &'static str,
    pub data_source: &'static str,
}

const DE: Strings = Strings {
    tab_search: "Suchen",
    tab_favourites: "Favoriten",
    tab_library: "Downloads",
    tab_settings: "Einstellungen",
    search_hint: "Was möchtest du sehen?",
    search: "Suchen",
    start_hint: "Tippe oben ein, was du sehen möchtest – zum Beispiel „Sendung mit der Maus“.",
    nothing_found: "Leider nichts gefunden. Probier ein anderes Wort!",
    search_failed: "Die Suche hat nicht geklappt. Bist du mit dem Internet verbunden?",
    minutes: "Min.",
    watch: "Anschauen",
    save: "Für später speichern",
    saved: "Gespeichert",
    saving: "Wird gespeichert…",
    no_favourites: "Noch keine Favoriten. Klicke beim Suchen auf den Stern, dann findest du das Video hier wieder.",
    no_downloads: "Noch nichts gespeichert. Gespeicherte Videos kannst du auch ohne Internet anschauen.",
    bookmark: "Zu Favoriten hinzufügen",
    unbookmark: "Aus Favoriten entfernen",
    delete: "Löschen",
    really_delete: "Wirklich löschen?",
    failed: "fehlgeschlagen",
    clear_finished: "Fertige ausblenden",
    close: "Schließen",
    cannot_play: "Dieses Video lässt sich leider nicht abspielen.",
    open_other_app: "Mit anderem Programm öffnen",
    fullscreen: "Vollbild (F, zurück mit Esc)",
    language: "Sprache",
    quality: "Bildqualität",
    quality_high: "Hoch",
    quality_medium: "Mittel",
    quality_low: "Niedrig",
    quality_note: "Höhere Qualität braucht mehr Speicherplatz und schnelleres Internet.",
    save_location: "Speicherort",
    open_folder: "Ordner öffnen",
    data_source: "Daten von mediathekviewweb.de – Sendungen der öffentlich-rechtlichen Sender.",
};

const EN: Strings = Strings {
    tab_search: "Search",
    tab_favourites: "Favourites",
    tab_library: "Downloads",
    tab_settings: "Settings",
    search_hint: "What do you want to watch?",
    search: "Search",
    start_hint: "Type what you'd like to watch above – for example “Sendung mit der Maus”.",
    nothing_found: "Nothing found. Try another word!",
    search_failed: "Search didn't work. Are you connected to the internet?",
    minutes: "min",
    watch: "Watch",
    save: "Save for later",
    saved: "Saved",
    saving: "Saving…",
    no_favourites: "No favourites yet. Click the star while searching and the video will show up here.",
    no_downloads: "Nothing saved yet. Saved videos can be watched without internet.",
    bookmark: "Add to favourites",
    unbookmark: "Remove from favourites",
    delete: "Delete",
    really_delete: "Really delete?",
    failed: "failed",
    clear_finished: "Hide finished",
    close: "Close",
    cannot_play: "Sorry, this video can't be played.",
    open_other_app: "Open with another app",
    fullscreen: "Fullscreen (F, Esc to leave)",
    language: "Language",
    quality: "Picture quality",
    quality_high: "High",
    quality_medium: "Medium",
    quality_low: "Low",
    quality_note: "Higher quality needs more disk space and faster internet.",
    save_location: "Save location",
    open_folder: "Open folder",
    data_source: "Data from mediathekviewweb.de – programmes of the German public broadcasters.",
};
