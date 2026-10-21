//! The shell actions the custom top bar and the tray share, and the one place a window action is
//! turned into a window operation.
//!
//! Tray menus and the webview's top bar are two ways to ask for the same four things. Keeping the
//! request as a plain value ([`ShellAction`]) and the performing behind a trait ([`ShellActions`])
//! means there is exactly one implementation of "hide" and one of "close", so the tray's quit and
//! the top bar's close button cannot drift into two different behaviours (A4), and the mapping can
//! be tested without a display, a tray, or a window.

/// One action the shell can perform on the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAction {
    Minimize,
    Hide,
    Show,
    Close,
}

/// The window operations the shell needs, abstracted so the decision can be tested on its own.
///
/// The production implementation talks to Tauri; the test double records what it was asked to do.
pub trait ShellActions {
    fn minimize(&self) -> Result<(), String>;
    fn hide(&self) -> Result<(), String>;
    fn show(&self) -> Result<(), String>;
    /// Quit the application, gracefully, through the same path for every caller.
    fn close(&self) -> Result<(), String>;
}

/// The tray menu id for each item. They are stable strings because the event handler matches on
/// them, and they survive a language change while the *words* do not: the menu is rebuilt from the
/// stored language, so the ids stay fixed and only the labels move. `quit` is deliberately the same
/// [`ShellAction`] the top bar's close button performs (A4). `settings` is the one id with no
/// [`ShellAction`]: it asks the interface to navigate instead of acting on the window (R12).
pub const MENU_SHOW: &str = "show";
pub const MENU_HIDE: &str = "hide";
pub const MENU_SETTINGS: &str = "settings";
pub const MENU_QUIT: &str = "quit";

/// The language tag the interface and the tray share. These are the same tags the webview's i18n
/// catalogue uses, because there is one stored value under `settings('language')` and both halves
/// must agree on it.
pub const LANGUAGE_EN: &str = "en";
pub const LANGUAGE_ES: &str = "es";

/// Every interface language this build knows, in the order the interface offers them.
///
/// The gate below and the refusal in `commands::set_language` both read THIS list, so the sentence a
/// person (or a log) receives can name exactly what the build accepts instead of saying "invalid". A
/// third list somewhere else is how a language gets added in the interface and forgotten in the tray.
pub const LANGUAGES: [&str; 2] = [LANGUAGE_EN, LANGUAGE_ES];

/// The tray menu's words in one language.
///
/// The tray is native and cannot read the webview's i18n catalogue, so its few words live here,
/// keyed by the stored language, instead of being spelled out at every menu rebuild. `show` and
/// `hide` name the application (R13); `settings` words the entry that asks the interface to
/// navigate (R12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayLabels {
    pub show: &'static str,
    pub hide: &'static str,
    pub settings: &'static str,
    pub quit: &'static str,
}

/// Whether `language` is a locale this build knows.
///
/// The store accepts any string under `settings`, so this is the one gate that keeps a typo — or a
/// value written by a newer build — from being treated as a language. Callers that read the setting
/// report an unknown value as no language; [`tray_labels`] falls back to English.
pub fn is_known_language(language: &str) -> bool {
    LANGUAGES.contains(&language)
}

/// The tray's words in `language`; English when nothing is stored or the value is not known.
pub fn tray_labels(language: Option<&str>) -> TrayLabels {
    match language {
        Some(LANGUAGE_ES) => TrayLabels {
            show: "Mostrar Mirk",
            hide: "Ocultar Mirk",
            settings: "Ajustes",
            quit: "Salir",
        },
        _ => TrayLabels {
            show: "Show Mirk",
            hide: "Hide Mirk",
            settings: "Settings",
            quit: "Quit",
        },
    }
}

/// One entry of the tray menu: the stable id the handler matches on, and the words in front of the
/// user. Keeping both together is what lets the whole menu be pinned by a test instead of only its
/// words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayMenuEntry {
    pub id: &'static str,
    pub label: &'static str,
}

/// One item of the tray menu: an actionable entry, or the line that divides one group from the next
/// (R15).
///
/// The divider is a value here rather than a call only the builder knows about, so the menu's whole
/// shape — entry, line, entry, line, entry — is a decision a test can pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayMenuItem {
    Entry(TrayMenuEntry),
    Divider,
}

/// The tray menu's items, in the order the user sees them, for `language` and a window that is
/// visible right now (R12, R13, R14, R15).
///
/// Exactly one visibility entry is returned — the one that can act on the window as it is — so the
/// menu never offers a click that does nothing. The opinion about a *minimized* window belongs to
/// the caller that reads the window, which reports a minimized window as not visible: the words are
/// chosen from that answer, never remembered beside it.
///
/// This is the whole decision the tray makes, as a plain function. It takes a `bool` rather than a
/// window so every combination can be pinned without a display, and the caller's only remaining job
/// is to ask the window and hand the answer in.
pub fn tray_menu_entries(language: Option<&str>, window_visible: bool) -> Vec<TrayMenuItem> {
    let labels = tray_labels(language);
    let visibility = if window_visible {
        TrayMenuEntry {
            id: MENU_HIDE,
            label: labels.hide,
        }
    } else {
        TrayMenuEntry {
            id: MENU_SHOW,
            label: labels.show,
        }
    };
    // The user asked for a line between each of the three groups, so the three entries are separated
    // by two dividers and never run together.
    vec![
        TrayMenuItem::Entry(visibility),
        TrayMenuItem::Divider,
        TrayMenuItem::Entry(TrayMenuEntry {
            id: MENU_SETTINGS,
            label: labels.settings,
        }),
        TrayMenuItem::Divider,
        TrayMenuItem::Entry(TrayMenuEntry {
            id: MENU_QUIT,
            label: labels.quit,
        }),
    ]
}

/// Perform `action` on `shell`.
pub fn dispatch(action: ShellAction, shell: &dyn ShellActions) -> Result<(), String> {
    match action {
        ShellAction::Minimize => shell.minimize(),
        ShellAction::Hide => shell.hide(),
        ShellAction::Show => shell.show(),
        ShellAction::Close => shell.close(),
    }
}

/// The shell action a tray menu id names, or `None` for an id this shell does not own.
pub fn action_for_menu_id(id: &str) -> Option<ShellAction> {
    match id {
        MENU_SHOW => Some(ShellAction::Show),
        MENU_HIDE => Some(ShellAction::Hide),
        MENU_QUIT => Some(ShellAction::Close),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records the operations it was asked to perform, in order.
    #[derive(Default)]
    struct FakeShell {
        calls: RefCell<Vec<ShellAction>>,
        fail_with: Option<&'static str>,
    }

    impl FakeShell {
        fn recording() -> Self {
            Self::default()
        }

        fn failing(message: &'static str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail_with: Some(message),
            }
        }

        fn call(&self, action: ShellAction) -> Result<(), String> {
            self.calls.borrow_mut().push(action);
            match self.fail_with {
                Some(message) => Err(message.to_string()),
                None => Ok(()),
            }
        }
    }

    impl ShellActions for FakeShell {
        fn minimize(&self) -> Result<(), String> {
            self.call(ShellAction::Minimize)
        }
        fn hide(&self) -> Result<(), String> {
            self.call(ShellAction::Hide)
        }
        fn show(&self) -> Result<(), String> {
            self.call(ShellAction::Show)
        }
        fn close(&self) -> Result<(), String> {
            self.call(ShellAction::Close)
        }
    }

    #[test]
    fn every_action_reaches_exactly_its_own_operation() {
        for action in [
            ShellAction::Minimize,
            ShellAction::Hide,
            ShellAction::Show,
            ShellAction::Close,
        ] {
            let shell = FakeShell::recording();
            dispatch(action, &shell).expect("the fake never fails");
            assert_eq!(&shell.calls.borrow()[..], &[action]);
        }
    }

    #[test]
    fn the_tray_menu_names_the_three_actions_and_nothing_else() {
        assert_eq!(action_for_menu_id(MENU_SHOW), Some(ShellAction::Show));
        assert_eq!(action_for_menu_id(MENU_HIDE), Some(ShellAction::Hide));
        assert_eq!(action_for_menu_id(MENU_QUIT), Some(ShellAction::Close));
        assert_eq!(action_for_menu_id("maximize"), None);
        assert_eq!(action_for_menu_id(""), None);
    }

    #[test]
    fn a_failing_window_operation_is_reported_not_swallowed() {
        let shell = FakeShell::failing("no window");
        let error = dispatch(ShellAction::Minimize, &shell).expect_err("the fake fails");
        assert_eq!(error, "no window");
    }

    #[test]
    fn the_tray_labels_follow_the_stored_language() {
        // The stored language, not the interface's in-memory state: the desktop process words the
        // tray (R1) and the words are pinned here so neither translation can drift. The visibility
        // entry names the application (R13), and Settings (R12) is a fourth word with no action.
        assert_eq!(
            tray_labels(Some("en")),
            TrayLabels {
                show: "Show Mirk",
                hide: "Hide Mirk",
                settings: "Settings",
                quit: "Quit",
            }
        );
        assert_eq!(
            tray_labels(Some("es")),
            TrayLabels {
                show: "Mostrar Mirk",
                hide: "Ocultar Mirk",
                settings: "Ajustes",
                quit: "Salir",
            }
        );
    }

    #[test]
    fn the_settings_entry_is_not_a_window_action() {
        // Settings asks the interface to navigate (R12); it must never become a window operation
        // by accident, which is why its id is not one `action_for_menu_id` knows.
        assert_eq!(action_for_menu_id(MENU_SETTINGS), None);
        assert_eq!(action_for_menu_id(MENU_SHOW), Some(ShellAction::Show));
        assert_eq!(action_for_menu_id(MENU_HIDE), Some(ShellAction::Hide));
        assert_eq!(action_for_menu_id(MENU_QUIT), Some(ShellAction::Close));
    }

    #[test]
    fn the_tray_menu_offers_one_visibility_entry_chosen_by_the_window() {
        let entry = |id: &'static str, label: &'static str| {
            TrayMenuItem::Entry(TrayMenuEntry { id, label })
        };
        let divider = TrayMenuItem::Divider;

        // All four combinations of the two things the menu depends on, in the order the user sees
        // them: the single visibility entry, a line, Settings, a line, Quit. Visible and hidden
        // differ only in that first entry, and never carry both (R14). The dividers are part of the
        // decision — not syntax sprinkled on at the builder — so the whole shape is pinned here
        // rather than only its words (R15).
        for (language, visible, visibility) in [
            ("en", true, (MENU_HIDE, "Hide Mirk")),
            ("en", false, (MENU_SHOW, "Show Mirk")),
            ("es", true, (MENU_HIDE, "Ocultar Mirk")),
            ("es", false, (MENU_SHOW, "Mostrar Mirk")),
        ] {
            let settings = if language == "es" { "Ajustes" } else { "Settings" };
            let quit = if language == "es" { "Salir" } else { "Quit" };
            assert_eq!(
                tray_menu_entries(Some(language), visible),
                vec![
                    entry(visibility.0, visibility.1),
                    divider,
                    entry(MENU_SETTINGS, settings),
                    divider,
                    entry(MENU_QUIT, quit),
                ],
                "language {language}, visible {visible}"
            );
        }
    }

    #[test]
    fn exactly_one_visibility_entry_is_offered_for_every_language_and_state() {
        // The invariant behind R14, over languages the build may or may not know and both window
        // states: three entries divided into three groups by two lines (R15), exactly one of
        // Show/Hide among them, and it is the one that can act on the window as it is.
        for language in [None, Some("en"), Some("es"), Some("fr")] {
            for visible in [true, false] {
                let items = tray_menu_entries(language, visible);

                let entries: Vec<TrayMenuEntry> = items
                    .iter()
                    .filter_map(|item| match item {
                        TrayMenuItem::Entry(entry) => Some(*entry),
                        TrayMenuItem::Divider => None,
                    })
                    .collect();
                assert_eq!(
                    entries.len(),
                    3,
                    "language {language:?}, visible {visible}: three entries"
                );

                let visibility: Vec<&str> = entries
                    .iter()
                    .filter(|entry| matches!(entry.id, MENU_SHOW | MENU_HIDE))
                    .map(|entry| entry.id)
                    .collect();
                assert_eq!(
                    visibility.len(),
                    1,
                    "language {language:?}, visible {visible}: exactly one visibility entry"
                );
                let expected = if visible { MENU_HIDE } else { MENU_SHOW };
                assert_eq!(visibility[0], expected);

                let dividers = items
                    .iter()
                    .filter(|item| matches!(item, TrayMenuItem::Divider))
                    .count();
                assert_eq!(
                    dividers,
                    2,
                    "language {language:?}, visible {visible}: a line between each entry"
                );
            }
        }
    }

    #[test]
    fn an_unknown_or_absent_language_falls_back_to_english() {
        assert!(is_known_language("en"));
        assert!(is_known_language("es"));
        assert!(!is_known_language("fr"));

        assert_eq!(tray_labels(None), tray_labels(Some("en")));
        assert_eq!(tray_labels(Some("fr")), tray_labels(Some("en")));
    }
}
