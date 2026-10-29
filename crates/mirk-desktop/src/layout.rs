//! The four layouts the window can take, and the size each one pins.
//!
//! The window is not resizable by hand (A2): its size is a stored choice, made in Ajustes → Ventana,
//! and this module is the only place that turns that choice into logical pixels. It is deliberately
//! free of Tauri and of the store: the desktop process applies the answer, the interface draws the
//! grid, and both halves name their layouts with the same keys.
//!
//! The widths are derived from one rule the reader stated after seeing the first attempt: a card keeps
//! the size it has with two per row, and the window grows instead. So the card's width is the one the
//! shipped 430 px window already gives it — `(430 − 32 padding − 9.6 gap) / 2 = 194.2` — and a row of
//! `n` cards needs `n × 194.2 + (n − 1) × 9.6` of content. That makes the two-column case the shipped
//! 430 px window exactly, three columns 634 px, and four columns 838 px; the cards themselves never
//! shrink to fit a wider row.

/// The four layouts the reader can choose, in the order Ajustes → Ventana offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowLayout {
    /// Two documents per row: the window the application shipped with.
    Columns2,
    /// Three per row.
    Columns3,
    /// Four per row.
    Columns4,
    /// Full screen, with as many documents per row as the card's minimum allows.
    Auto,
}

/// The layout in force when nothing is stored: full screen (R7 of `desktop-chrome.md`).
pub const DEFAULT_LAYOUT: WindowLayout = WindowLayout::Auto;

/// The windowed layout a window comes back to when nothing usable is stored: three per row (R7, S3).
///
/// It is the reader's own answer to "three per row if there is none", and it is read at runtime now: it
/// is what [`preset`] falls back to, and so what a window that leaves full screen takes when the reader
/// never chose a preset. It also keeps the declared launch size in `tauri.conf.json` tied to a real
/// layout — the window is created at the size "restore" comes back to, and `setup` then fills the work
/// area — and a test asserts the two numbers are one, so they cannot drift apart into a launch frame the
/// reader can never recover.
pub const RESTORE_LAYOUT: WindowLayout = WindowLayout::Columns3;

/// The key the choice is stored under in the `settings` table. Both halves read this value.
pub const LAYOUT_SETTING: &str = "window_layout";

/// The key the reader's last windowed layout is stored under (S3).
///
/// It is a second setting beside [`LAYOUT_SETTING`] because full screen is not a preset: choosing it
/// must leave the rows the reader last chose untouched, or dragging out of full screen would have
/// nothing to come back to. Only `columns-2`, `columns-3` and `columns-4` are ever written here.
pub const PRESET_SETTING: &str = "window_preset";

/// Every layout key, in the order the interface offers them.
pub const LAYOUT_KEYS: [&str; 4] = ["columns-2", "columns-3", "columns-4", "auto"];

/// The fixed height of every windowed layout: the phone shape the user asked for and did not reopen.
pub const WINDOW_HEIGHT: f64 = 844.0;

/// The grid's gap: `0.6rem`.
const GRID_GAP: f64 = 0.6 * 16.0;

/// The shell's horizontal padding: `--space-4` on each side.
const SHELL_PADDING: f64 = 2.0 * 16.0;

/// The narrowest window the application ever takes, which is its shipped width.
const PHONE_WIDTH: f64 = 430.0;

/// One card's width: the width two columns already have in the shipped window.
///
/// It is derived from that window rather than declared, because the rule is "the cards keep the size
/// they have with two per row": a number copied from the stylesheet could drift from what the reader
/// actually sees, while this one cannot.
const CARD_WIDTH: f64 = (PHONE_WIDTH - SHELL_PADDING - GRID_GAP) / 2.0;

impl WindowLayout {
    /// The stored key for this layout.
    pub fn key(self) -> &'static str {
        match self {
            Self::Columns2 => "columns-2",
            Self::Columns3 => "columns-3",
            Self::Columns4 => "columns-4",
            Self::Auto => "auto",
        }
    }

    /// The layout a stored value names, or `None` when this build does not know it.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "columns-2" => Some(Self::Columns2),
            "columns-3" => Some(Self::Columns3),
            "columns-4" => Some(Self::Columns4),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    /// The documents a row holds, or `None` for full screen, which has no fixed count.
    pub fn columns(self) -> Option<u32> {
        match self {
            Self::Columns2 => Some(2),
            Self::Columns3 => Some(3),
            Self::Columns4 => Some(4),
            Self::Auto => None,
        }
    }

    /// The logical size this layout pins, or `None` when the layout is full screen.
    ///
    /// The width is the row the layout promises at the card's own size, rounded up because a window one
    /// pixel short of its row would show one card fewer than the layout names.
    pub fn window_size(self) -> Option<(f64, f64)> {
        let columns = self.columns()? as f64;
        let content = columns * CARD_WIDTH + (columns - 1.0) * GRID_GAP;
        Some(((content + SHELL_PADDING).ceil(), WINDOW_HEIGHT))
    }
}

/// The preset a stored `window_preset` value names, or [`RESTORE_LAYOUT`] when it names none.
///
/// A value this build does not know and a stored `auto` are both *nothing stored*: full screen is not a
/// preset, so it cannot be the answer to "which rows do I come back to", and neither can a word an older
/// or a newer build left behind. Three per row is what is left, which is the default the reader asked
/// for. `None` is an absent key, the same answer as an unusable value.
pub fn preset(stored: Option<&str>) -> WindowLayout {
    stored
        .and_then(WindowLayout::parse)
        .filter(|layout| layout.columns().is_some())
        .unwrap_or(RESTORE_LAYOUT)
}

/// What the stored layout must become when the window's real shape disagrees with it, if anything.
///
/// The window manager's own drag gesture changes the window behind the application's back (S2): a
/// maximised window dragged by its bar is unmaximised by the compositor while the stored layout still
/// says `auto`, so the interface goes on drawing the sidebar, the pressed maximise control and the
/// auto-filling grid over a window that is no longer full screen. The rule is that the window's shape
/// wins — when the window is not maximised and the store still says full screen, the layout becomes the
/// reader's `preset` — and `None` says the two already agree, so the caller does nothing.
///
/// `preset` is an argument rather than something read here, so the rule is a comparison and nothing else:
/// its four cases can be asserted without a store or a window. Only the `auto` direction moves. A
/// maximised window whose store names a preset is not corrected, because the window manager is allowed to
/// be briefly out of step while a layout is being applied, and a rule that fought that would turn every
/// apply into a loop.
pub fn reconcile(
    stored: WindowLayout,
    maximised: bool,
    preset: WindowLayout,
) -> Option<WindowLayout> {
    if stored == WindowLayout::Auto && !maximised {
        Some(preset)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_layout_parses_and_round_trips() {
        for key in LAYOUT_KEYS {
            let layout = WindowLayout::parse(key).unwrap_or_else(|| panic!("`{key}` must parse"));
            assert_eq!(layout.key(), key, "a layout's key is the one it parsed from");
        }
    }

    #[test]
    fn a_value_this_build_does_not_know_is_not_a_layout() {
        for value in ["", "columns-5", "columns-1", "3", "Columns-3", "auto-fill", "grid"] {
            assert_eq!(WindowLayout::parse(value), None, "`{value}` must not parse");
        }
    }

    #[test]
    fn the_default_is_full_screen() {
        assert_eq!(DEFAULT_LAYOUT, WindowLayout::Auto);
        assert_eq!(
            DEFAULT_LAYOUT.window_size(),
            None,
            "R7: full screen is the work area, not a size table entry"
        );
    }

    #[test]
    fn the_window_that_restore_returns_to_is_three_per_row() {
        assert_eq!(RESTORE_LAYOUT, WindowLayout::Columns3);
        assert_eq!(RESTORE_LAYOUT.window_size(), Some((634.0, WINDOW_HEIGHT)));
        assert_eq!(preset(None), RESTORE_LAYOUT, "nothing stored is the fallback");
    }

    // --- S3: the stored preset -----------------------------------------------------------------

    #[test]
    fn a_stored_preset_is_the_windowed_layout_it_names() {
        for (stored, expected) in [
            ("columns-2", WindowLayout::Columns2),
            ("columns-3", WindowLayout::Columns3),
            ("columns-4", WindowLayout::Columns4),
        ] {
            assert_eq!(preset(Some(stored)), expected, "`{stored}` names {expected:?}");
        }
    }

    #[test]
    fn full_screen_is_never_a_preset() {
        assert_eq!(
            preset(Some("auto")),
            RESTORE_LAYOUT,
            "full screen cannot be the rows a window comes back to, so it reads as nothing stored"
        );
        assert!(preset(Some("auto")).columns().is_some(), "a preset always counts its columns");
    }

    #[test]
    fn a_stored_preset_this_build_does_not_know_and_a_missing_one_both_fall_back_to_three() {
        for stored in [None, Some(""), Some("columns-5"), Some("Columns-3"), Some("auto-fill")] {
            assert_eq!(
                preset(stored),
                RESTORE_LAYOUT,
                "`{stored:?}` is not a preset this build knows, so three per row stands"
            );
        }
    }

    // --- S2: the rule that reconciles the window's shape with the store -------------------------

    #[test]
    fn a_preset_that_the_window_agrees_with_is_not_reconciled() {
        assert_eq!(
            reconcile(WindowLayout::Columns4, true, RESTORE_LAYOUT),
            None,
            "a maximised window over a stored preset is the window manager settling, not a drag"
        );
        assert_eq!(
            reconcile(WindowLayout::Columns4, false, RESTORE_LAYOUT),
            None,
            "a windowed window over a stored preset already agrees with itself"
        );
    }

    #[test]
    fn full_screen_that_the_window_still_fills_is_not_reconciled() {
        assert_eq!(
            reconcile(WindowLayout::Auto, true, RESTORE_LAYOUT),
            None,
            "full screen over a maximised window is exactly what was asked for"
        );
    }

    #[test]
    fn a_window_that_left_full_screen_takes_the_preset_that_was_passed_in() {
        assert_eq!(
            reconcile(WindowLayout::Auto, false, WindowLayout::Columns4),
            Some(WindowLayout::Columns4),
            "the reader's last windowed choice is what the store must become"
        );
        assert_eq!(
            reconcile(WindowLayout::Auto, false, RESTORE_LAYOUT),
            Some(RESTORE_LAYOUT),
            "with no preset stored, three per row is the answer"
        );
    }

    #[test]
    fn each_preset_names_its_columns_and_full_screen_names_none() {
        assert_eq!(WindowLayout::Columns2.columns(), Some(2));
        assert_eq!(WindowLayout::Columns3.columns(), Some(3));
        assert_eq!(WindowLayout::Columns4.columns(), Some(4));
        assert_eq!(WindowLayout::Auto.columns(), None);
    }

    #[test]
    fn each_preset_pins_the_size_the_spec_names() {
        // S2's table, as the desktop process applies it: the shipped window for two columns, and the
        // derived 633.8 / 837.6 rounded up for three and four.
        assert_eq!(WindowLayout::Columns2.window_size(), Some((430.0, 844.0)));
        assert_eq!(WindowLayout::Columns3.window_size(), Some((634.0, 844.0)));
        assert_eq!(WindowLayout::Columns4.window_size(), Some((838.0, 844.0)));
    }

    /// L7: the cards keep the size they have with two per row. The window is what grows.
    #[test]
    fn every_preset_gives_its_cards_the_two_per_row_width() {
        let two = WindowLayout::Columns2.window_size().expect("a windowed layout");
        let two_card = (two.0 - SHELL_PADDING - GRID_GAP) / 2.0;

        for key in ["columns-2", "columns-3", "columns-4"] {
            let layout = WindowLayout::parse(key).expect("a preset");
            let (width, height) = layout.window_size().expect("a windowed layout has a size");
            assert_eq!(height, WINDOW_HEIGHT, "the height stays the phone's");

            let columns = f64::from(layout.columns().expect("a preset counts its columns"));
            let content = width - SHELL_PADDING;
            let card = (content - (columns - 1.0) * GRID_GAP) / columns;
            assert!(
                (card - two_card).abs() < 0.5,
                "`{key}` gives each card {card} px; two per row gives {two_card} px, so the cards shrank"
            );
            assert!(
                width >= PHONE_WIDTH,
                "`{key}` must never shrink below the shipped window: {width}"
            );
            assert!(
                content - (columns * card + (columns - 1.0) * GRID_GAP) < 1.0,
                "`{key}` is wider than its own row needs: {width}"
            );
        }
    }

    #[test]
    fn full_screen_pins_no_size_because_the_screen_decides_one() {
        assert_eq!(WindowLayout::Auto.window_size(), None);
    }
}
