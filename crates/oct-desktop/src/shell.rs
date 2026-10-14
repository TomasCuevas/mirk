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
/// [`ShellAction`] the top bar's close button performs (A4).
pub const MENU_SHOW: &str = "show";
pub const MENU_HIDE: &str = "hide";
pub const MENU_QUIT: &str = "quit";

/// The language tag the interface and the tray share. These are the same tags the webview's i18n
/// catalogue uses, because there is one stored value under `settings('language')` and both halves
/// must agree on it.
pub const LANGUAGE_EN: &str = "en";
pub const LANGUAGE_ES: &str = "es";

/// The tray menu's words in one language.
///
/// The tray is native and cannot read the webview's i18n catalogue, so its few words live here,
/// keyed by the stored language, instead of being spelled out at every menu rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayLabels {
    pub show: &'static str,
    pub hide: &'static str,
    pub quit: &'static str,
}

/// Whether `language` is a locale this build knows.
///
/// The store accepts any string under `settings`, so this is the one gate that keeps a typo — or a
/// value written by a newer build — from being treated as a language. Callers that read the setting
/// report an unknown value as no language; [`tray_labels`] falls back to English.
pub fn is_known_language(language: &str) -> bool {
    matches!(language, LANGUAGE_EN | LANGUAGE_ES)
}

/// The tray's words in `language`; English when nothing is stored or the value is not known.
pub fn tray_labels(language: Option<&str>) -> TrayLabels {
    match language {
        Some(LANGUAGE_ES) => TrayLabels {
            show: "Mostrar",
            hide: "Ocultar",
            quit: "Salir",
        },
        _ => TrayLabels {
            show: "Show",
            hide: "Hide",
            quit: "Quit",
        },
    }
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
        // tray (R1) and the words are pinned here so neither translation can drift.
        assert_eq!(
            tray_labels(Some("en")),
            TrayLabels {
                show: "Show",
                hide: "Hide",
                quit: "Quit",
            }
        );
        assert_eq!(
            tray_labels(Some("es")),
            TrayLabels {
                show: "Mostrar",
                hide: "Ocultar",
                quit: "Salir",
            }
        );
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
