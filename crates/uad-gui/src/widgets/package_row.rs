use crate::style;
use crate::theme::Theme;
use crate::views::settings::Settings;
use crate::widgets::text;
use log::warn;
use std::fmt::Write as _;
use uad_core::sync::{CorePackage, Phone};
use uad_core::uad_lists::{PackageState, Removal, UadList};

use iced::widget::{Space, button, checkbox, column, row};
use iced::{Alignment, Element, Length, Renderer, Task, alignment};

#[derive(Clone, Debug)]
pub struct PackageRow {
    pub name: String,
    /// Human-readable app name (from AI enrichment or curated data). Empty when
    /// unknown — the row then falls back to [`prettify_pkg_id`] of `name`.
    pub friendly_name: String,
    pub description: String,
    pub removal: Removal,
    pub state: PackageState,
    pub list: UadList,
    pub selected: bool,
    pub current: bool,
    /// `false` for third-party (user-installed) apps like Netflix; `true` for
    /// system packages. Set during load from `pm list packages -3`.
    pub is_system: bool,
}

/// Id segments that describe plumbing rather than an app identity — skipped when
/// deriving a label, so `com.google.android.apps.photos` reads as `Photos`.
const GENERIC_SEGMENTS: &[&str] = &[
    "android",
    "app",
    "apps",
    "application",
    "client",
    "core",
    "main",
    "mobile",
    "module",
    "package",
    "provider",
    "release",
    "service",
    "services",
    "ui",
];

/// Segments naming *which variant* of an app this is. They become a
/// parenthesised suffix instead of the label itself, so `…youtube.tv` reads as
/// `YouTube (TV)` rather than `Tv`.
const VARIANT_SEGMENTS: &[(&str, &str)] = &[
    ("tv", "TV"),
    ("wear", "Wear"),
    ("auto", "Auto"),
    ("tablet", "Tablet"),
    ("phone", "Phone"),
    ("go", "Go"),
    ("lite", "Lite"),
];

/// Segments that are opaque internal names with a well-known meaning.
/// Deliberately short and best-effort — AI enrichment supplies real names.
const KNOWN_SEGMENTS: &[(&str, &str)] = &[
    ("gms", "Google Play services"),
    ("gsf", "Google Services Framework"),
    ("vending", "Google Play Store"),
    ("systemui", "System UI"),
    ("packageinstaller", "Package Installer"),
    ("setupwizard", "Setup Wizard"),
    ("inputmethod", "Input Method"),
    ("webview", "WebView"),
    ("youtube", "YouTube"),
    ("gmail", "Gmail"),
    ("tts", "Text-to-Speech"),
    ("ims", "IMS"),
    ("nfc", "NFC"),
];

/// Split a glued variant prefix, e.g. `tvmusic` -> `("TV", "music")`.
///
/// Only `tv` is handled: Android TV packages glue it constantly, and no common
/// English word starts with it. Wider prefixes would mangle real segments
/// (`autofill` -> "Fill (Auto)", `wearable` -> "Able (Wear)").
fn split_variant_prefix(seg: &str) -> Option<(&'static str, &str)> {
    let rest = seg.strip_prefix("tv")?;
    (rest.len() >= 3).then_some(("TV", rest))
}

/// Best-effort human label derived from a package id, used when no AI or curated
/// friendly name is available.
///
/// Takes the last *meaningful* segment rather than simply the last one, so
/// `com.google.android.youtube.tv` reads as `YouTube (TV)` instead of `Tv`.
/// This is only a placeholder: ids whose leaf is an internal codename
/// (`com.netflix.ninja` -> `Ninja`) still need AI enrichment to become the real
/// app name, which is why the raw id stays visible next to the label.
#[must_use]
pub fn prettify_pkg_id(id: &str) -> String {
    let mut segments = id.split('.');
    // The leading reverse-domain TLD (`com`, `org`, …) is never part of a name.
    let tld = segments.next().unwrap_or(id);
    let rest: Vec<&str> = segments.collect();
    let Some(&last_segment) = rest.last() else {
        return capitalize_words(tld);
    };

    // `bool` marks a word that came from a glued variant prefix (`tvmusic`),
    // which reads correctly only when joined to the word before it.
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut variant: Option<&'static str> = None;
    for seg in &rest {
        let lower = seg.to_ascii_lowercase();
        if GENERIC_SEGMENTS.contains(&lower.as_str()) {
            continue;
        }
        if let Some(v) = VARIANT_SEGMENTS
            .iter()
            .find_map(|(k, v)| (*k == lower).then_some(*v))
        {
            variant = Some(v);
            continue;
        }
        if let Some((v, tail)) = split_variant_prefix(&lower) {
            variant = Some(v);
            words.push((tail.to_string(), true));
            continue;
        }
        words.push(((*seg).to_string(), false));
    }

    // Everything was generic or a variant (`com.android.phone`): the raw last
    // segment is all we have, and a "(Phone)" suffix on it would be nonsense.
    let Some((last_word, from_split)) = words.last() else {
        return capitalize_words(last_segment);
    };

    let mut label = String::new();
    if *from_split && words.len() >= 2 {
        label.push_str(&pretty_word(&words[words.len() - 2].0));
        label.push(' ');
    }
    label.push_str(&pretty_word(last_word));
    if let Some(v) = variant {
        let _ = write!(label, " ({v})");
    }
    label
}

/// Expand a known internal segment, else title-case it.
fn pretty_word(word: &str) -> String {
    KNOWN_SEGMENTS
        .iter()
        .find_map(|(k, v)| (*k == word.to_ascii_lowercase()).then_some((*v).to_string()))
        .unwrap_or_else(|| capitalize_words(word))
}

/// Title-case one id segment, splitting on `_`, `-` and camelCase boundaries.
fn capitalize_words(segment: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in segment.chars() {
        if ch == '_' || ch == '-' {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        // Split camelCase boundaries (a|B).
        if ch.is_uppercase() && prev_lower && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
        prev_lower = ch.is_lowercase();
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    if words.is_empty() {
        return segment.to_string();
    }
    words
        .iter()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug)]
pub enum Message {
    PackagePressed,
    ToggleSelection(bool),
    /// Flip this package between enabled and disabled, in place.
    ToggleState,
}

impl PackageRow {
    #[must_use]
    pub fn new(
        name: &str,
        description: &str,
        removal: Removal,
        state: PackageState,
        list: UadList,
        selected: bool,
        current: bool,
    ) -> Self {
        Self {
            name: name.to_string(),
            friendly_name: String::new(),
            description: description.to_string(),
            removal,
            state,
            list,
            selected,
            current,
            is_system: true,
        }
    }

    #[allow(
        clippy::unused_self,
        reason = "Consistent component API; may change later"
    )]
    pub fn update(&mut self, _message: &Message) -> Task<Message> {
        Task::none()
    }

    /// App name (prominent) above the raw package id (dimmed), plus the first
    /// line of the description when there is a real one. The name is the AI /
    /// curated `friendly_name` when known, else a label derived from the id.
    fn name_cell(&self) -> Element<'_, Message, Theme, Renderer> {
        let display_name = if self.friendly_name.is_empty() {
            prettify_pkg_id(&self.name)
        } else {
            self.friendly_name.clone()
        };
        let name_line = row![
            text(display_name).size(16),
            text(&self.name).size(12).style(style::Text::Commentary),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // The unknown-package placeholder is suppressed so those rows stay compact.
        let has_desc =
            !self.description.is_empty() && !self.description.starts_with("[No description]");
        if has_desc {
            let one_line = self
                .description
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            column![
                name_line,
                text(one_line).size(12).style(style::Text::Commentary),
            ]
            .spacing(2)
            .width(Length::FillPortion(8))
            .into()
        } else {
            column![name_line].width(Length::FillPortion(8)).into()
        }
    }

    pub fn view(
        &self,
        settings: &Settings,
        phone: &Phone,
    ) -> Element<'_, Message, Theme, Renderer> {
        //let trash_svg = format!("{}/resources/assets/trash.svg", env!("CARGO_MANIFEST_DIR"));
        //let restore_svg = format!("{}/resources/assets/rotate.svg", env!("CARGO_MANIFEST_DIR"));

        // Unsafe packages stay untouchable unless expert mode is on.
        let unlocked = self.removal != Removal::Unsafe
            || self.state != PackageState::Enabled
            || settings.general.expert_mode;

        // Current state, doubling as a one-click enabled <-> disabled toggle.
        // Uninstalling is deliberately NOT reachable from here — that stays
        // behind select-then-apply, so a stray click can't remove an app.
        let (state_text, state_style): (_, fn(&Theme) -> iced::widget::text::Style) =
            match self.state {
                PackageState::Enabled => ("Enabled", style::Text::Ok),
                PackageState::Disabled => ("Disabled", style::Text::Danger),
                PackageState::Uninstalled => ("Uninstalled", style::Text::Commentary),
                PackageState::All => {
                    warn!("Incredible! Something impossible happened!");
                    ("Error", style::Text::Danger)
                }
            };
        // Disabling needs `pm disable-user` (Android 6+); re-enabling always works.
        let toggleable = unlocked
            && match self.state {
                PackageState::Enabled => phone.android_sdk >= 23,
                PackageState::Disabled => true,
                PackageState::Uninstalled | PackageState::All => false,
            };
        let state_cell = {
            let btn = button(
                text(state_text)
                    .size(14)
                    .style(state_style)
                    .align_x(alignment::Horizontal::Right)
                    .width(Length::Fill),
            )
            .width(100)
            .padding(0)
            .style(style::Button::Hidden);
            if toggleable {
                btn.on_press(Message::ToggleState)
            } else {
                btn
            }
        };

        let selection_checkbox = checkbox(self.selected)
            .on_toggle(Message::ToggleSelection)
            .size(20)
            .style(if unlocked {
                style::CheckBox::PackageEnabled
            } else {
                style::CheckBox::PackageDisabled
            });

        let name_cell = self.name_cell();

        row![
            button(
                row![selection_checkbox, name_cell, state_cell]
                    .spacing(8)
                    .align_y(Alignment::Center)
            )
            .padding(8)
            .style(if self.current {
                style::Button::SelectedPackage
            } else {
                style::Button::NormalPackage
            })
            .width(Length::Fill)
            .on_press(Message::PackagePressed),
            Space::new().width(Length::Fixed(15.0))
        ]
        .align_y(Alignment::Center)
        .into()
    }
}

// Conversions between PackageRow and CorePackage
impl From<CorePackage> for PackageRow {
    fn from(core: CorePackage) -> Self {
        Self {
            name: core.name.clone(),
            friendly_name: String::new(),
            description: core.description,
            removal: core.removal,
            state: core.state,
            list: core.list,
            selected: false, // Default to not selected
            current: false,  // Default to not current
            is_system: true,
        }
    }
}

impl From<&CorePackage> for PackageRow {
    fn from(core: &CorePackage) -> Self {
        Self {
            name: core.name.clone(),
            friendly_name: String::new(),
            description: core.description.clone(),
            removal: core.removal,
            state: core.state,
            list: core.list,
            selected: false,
            current: false,
            is_system: true,
        }
    }
}

impl From<&PackageRow> for CorePackage {
    fn from(row: &PackageRow) -> Self {
        Self {
            name: row.name.clone(),
            description: row.description.clone(),
            removal: row.removal,
            state: row.state,
            list: row.list,
        }
    }
}

impl From<PackageRow> for CorePackage {
    fn from(row: PackageRow) -> Self {
        Self {
            name: row.name,
            description: row.description,
            removal: row.removal,
            state: row.state,
            list: row.list,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::prettify_pkg_id;

    #[test]
    fn variant_suffix_beats_bare_last_segment() {
        // The regression this exists for: `…youtube.tv` used to render as "Tv".
        assert_eq!(
            prettify_pkg_id("com.google.android.youtube.tv"),
            "YouTube (TV)"
        );
        assert_eq!(
            prettify_pkg_id("com.google.android.youtube.tvmusic"),
            "YouTube Music (TV)"
        );
        assert_eq!(prettify_pkg_id("com.android.tv.settings"), "Settings (TV)");
    }

    #[test]
    fn generic_segments_are_skipped() {
        assert_eq!(prettify_pkg_id("com.google.android.apps.photos"), "Photos");
        assert_eq!(prettify_pkg_id("com.google.android.backdrop"), "Backdrop");
    }

    #[test]
    fn known_segments_expand() {
        assert_eq!(
            prettify_pkg_id("com.google.android.gms"),
            "Google Play services"
        );
        assert_eq!(prettify_pkg_id("com.android.vending"), "Google Play Store");
        assert_eq!(prettify_pkg_id("com.google.android.tts"), "Text-to-Speech");
        assert_eq!(prettify_pkg_id("com.android.systemui"), "System UI");
    }

    #[test]
    fn falls_back_when_nothing_meaningful_is_left() {
        // Every segment is generic or a variant — the raw leaf is all we have,
        // and it must not gain a "(Phone)" suffix.
        assert_eq!(prettify_pkg_id("com.android.phone"), "Phone");
        assert_eq!(prettify_pkg_id("android"), "Android");
    }

    #[test]
    fn camel_and_separator_splitting_still_works() {
        assert_eq!(prettify_pkg_id("com.lonelycatgames.Xplore"), "Xplore");
        assert_eq!(prettify_pkg_id("com.example.myCoolThing"), "My Cool Thing");
        assert_eq!(prettify_pkg_id("com.example.two_words"), "Two Words");
    }
}
