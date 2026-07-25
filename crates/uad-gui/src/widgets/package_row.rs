use crate::style;
use crate::theme::Theme;
use crate::views::settings::Settings;
use crate::widgets::text;
use log::warn;
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

/// Best-effort human label derived from a package id, used when no AI or curated
/// friendly name is available. `com.google.android.backdrop` -> `Backdrop`.
#[must_use]
pub fn prettify_pkg_id(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in last.chars() {
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
        return last.to_string();
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
    ActionPressed,
    ToggleSelection(bool),
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

    pub fn view(
        &self,
        settings: &Settings,
        _phone: &Phone,
    ) -> Element<'_, Message, Theme, Renderer> {
        //let trash_svg = format!("{}/resources/assets/trash.svg", env!("CARGO_MANIFEST_DIR"));
        //let restore_svg = format!("{}/resources/assets/rotate.svg", env!("CARGO_MANIFEST_DIR"));
        let button_style: fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style;
        let action_text;
        let action_btn;
        let selection_checkbox;

        match self.state {
            PackageState::Enabled => {
                action_text = if settings.device.disable_mode {
                    "Disable"
                } else {
                    "Uninstall"
                };
                button_style = style::Button::UninstallPackage;
            }
            PackageState::Disabled => {
                action_text = "Enable";
                button_style = style::Button::RestorePackage;
            }
            PackageState::Uninstalled => {
                action_text = "Restore";
                button_style = style::Button::RestorePackage;
            }
            PackageState::All => {
                action_text = "Error";
                button_style = style::Button::RestorePackage;
                warn!("Incredible! Something impossible happened!");
            }
        }
        // Disable any removal action for unsafe packages if expert_mode is disabled
        if self.removal != Removal::Unsafe
            || self.state != PackageState::Enabled
            || settings.general.expert_mode
        {
            selection_checkbox = checkbox(self.selected)
                .on_toggle(Message::ToggleSelection)
                .size(20)
                .style(style::CheckBox::PackageEnabled);

            action_btn = button(
                text(action_text)
                    .align_x(alignment::Horizontal::Center)
                    .width(100),
            )
            .on_press(Message::ActionPressed);
        } else {
            selection_checkbox = checkbox(self.selected)
                .on_toggle(Message::ToggleSelection)
                .size(20)
                .style(style::CheckBox::PackageDisabled);

            action_btn = button(
                text(action_text)
                    .align_x(alignment::Horizontal::Center)
                    .width(100),
            );
        }

        // Friendly name (prominent) followed by the raw package id (dimmed,
        // smaller). Falls back to a prettified id when no friendly name is known.
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

        // Optional inline description (first line only, dimmed). The unknown-package
        // placeholder is suppressed so those rows stay compact.
        let has_desc =
            !self.description.is_empty() && !self.description.starts_with("[No description]");
        let name_cell: Element<'_, Message, Theme, Renderer> = if has_desc {
            let one_line = self.description.lines().next().unwrap_or_default().to_string();
            column![
                name_line,
                text(one_line).size(12).style(style::Text::Commentary),
            ]
            .spacing(2)
            .width(Length::FillPortion(8))
            .into()
        } else {
            column![name_line].width(Length::FillPortion(8)).into()
        };

        row![
            button(
                row![
                    selection_checkbox,
                    name_cell,
                    action_btn.style(button_style)
                ]
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
