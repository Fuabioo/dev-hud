use iced::widget::text::Shaping;
use iced::widget::{column, container, mouse_area, row, scrollable, space, text};
use iced::{mouse, Element, Length};

use crate::app::{Hud, Message};
use crate::session::*;
use crate::util;

impl Hud {
    pub(crate) fn view_modal(&self, modal: &ModalState) -> Element<'_, Message> {
        let mono = self.current_font();
        let shaped = Shaping::Advanced;
        let colors = &self.colors;

        let claude = match self.active_claude() {
            Some(c) => c,
            None => {
                return container(text("No data"))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(colors.modal_bg_style())
                    .into();
            }
        };

        let session = &claude.sessions[modal.session_index];
        let entries = &claude.activity_logs[modal.session_index];

        // Title row
        let title = text(format!(
            "{} {} \u{2014} Activity Log",
            session.kind.icon(true),
            util::shorten_project(&session.project_slug)
        ))
        .size(colors.modal_title)
        .color(colors.marker)
        .font(mono)
        .shaping(shaped);

        let entry_count = text(format!("{} entries", entries.len()))
            .size(colors.modal_text)
            .color(colors.muted)
            .font(mono)
            .shaping(shaped);

        let close_btn = mouse_area(
            text("\u{f00d}")
                .size(colors.modal_title)
                .color(colors.marker)
                .font(mono)
                .shaping(shaped),
        )
        .on_press(Message::CloseModal)
        .interaction(mouse::Interaction::Pointer);

        // Live-mode pulse indicator — reads spinner_frame so view_modal depends on it,
        // causing iced to re-render the modal surface on every Tick and pick up new
        // WatcherEvent entries in real time.
        let live_badge: Element<'_, Message> = if self.claude.is_some() {
            let frames = &["◉", "◎", "○", "◎"];
            let pulse = frames[(claude.spinner_frame / 8) % frames.len()];
            text(format!("  {pulse} live"))
                .size(colors.modal_text)
                .color(colors.hover_text)
                .font(mono)
                .shaping(shaped)
                .into()
        } else {
            space::horizontal().into()
        };

        let title_row = row![title, live_badge, text("  "), entry_count, space::horizontal(), close_btn];

        // UUID subtitle row with copy button
        let uuid_text = text(format!("  {}", session.session_id))
            .size(colors.modal_text * 0.9)
            .color(colors.muted)
            .font(mono)
            .shaping(shaped);

        let copy_btn = mouse_area(
            text("\u{f0c5}") // nf-fa-copy
                .size(colors.modal_text)
                .color(colors.muted)
                .font(mono)
                .shaping(shaped),
        )
        .on_press(Message::CopySessionId(session.session_id.clone()))
        .interaction(mouse::Interaction::Pointer);

        let uuid_row = row![uuid_text, text(" "), copy_btn];

        // --- Left panel: scrollable entry list ---
        let mut entries_col = column![].spacing(2);

        for (i, entry) in entries.iter().enumerate() {
            let is_selected = modal.selected_entry == Some(i);
            let is_hovered = !is_selected && modal.hovered_entry == Some(i);

            // Guardrail-blocked entries carry the \u{f071} warning triangle in their summary.
            // Distinguish them from genuine tool failures so each gets its own accent.
            let is_guardrail = entry.is_error && entry.summary.contains('\u{f071}');
            let is_genuine_error = entry.is_error && !is_guardrail;
            // Last entry while a tool is actively running = awaiting approval / in progress.
            let is_active =
                !entry.is_error && session.current_tool.is_some() && i == entries.len() - 1;

            let accent = if is_genuine_error {
                Some(colors.error)
            } else if is_guardrail || is_active {
                Some(colors.approval)
            } else {
                None
            };

            let fg = match (accent, is_hovered) {
                (Some(c), _) => c,
                (None, true) => colors.hover_text,
                (None, false) => colors.marker,
            };
            let dim = match (accent, is_hovered) {
                (Some(c), _) => c,
                (None, true) => colors.hover_text,
                (None, false) => colors.muted,
            };

            // Genuine errors get ✘ prefix; guardrail blocks already carry \u{f071} in summary;
            // active/in-progress entries get a subtle ⋯ indicator.
            let icon_prefix = if is_genuine_error {
                "✘ "
            } else if is_active {
                "⋯ "
            } else {
                ""
            };

            let entry_row = row![
                text(format!("{} ", entry.timestamp))
                    .size(colors.modal_text)
                    .color(dim)
                    .font(mono)
                    .shaping(shaped),
                text(format!("{icon_prefix}{:<5} ", entry.tool))
                    .size(colors.modal_text)
                    .color(fg)
                    .font(mono)
                    .shaping(shaped),
                text(util::truncate_str(&entry.summary, 48))
                    .size(colors.modal_text)
                    .color(if is_selected { colors.marker } else { dim })
                    .font(mono)
                    .shaping(shaped),
            ];

            let entry_element: Element<'_, Message> = if is_selected {
                container(entry_row)
                    .style(colors.selected_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else if is_hovered {
                container(entry_row)
                    .style(colors.hover_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else {
                container(entry_row)
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            };

            entries_col = entries_col.push(
                mouse_area(entry_element)
                    .on_press(Message::SelectActivity(i))
                    .on_enter(Message::HoverEntry(i))
                    .on_exit(Message::UnhoverEntry(i))
                    .interaction(mouse::Interaction::Pointer),
            );
        }

        let left_panel = scrollable(entries_col)
            .width(Length::FillPortion(2))
            .height(Length::Fill);

        // --- Right panel: detail view ---
        let right_panel: Element<'_, Message> = if let Some(idx) = modal.selected_entry {
            let entry = &entries[idx];

            let detail_is_guardrail =
                entry.is_error && entry.summary.contains('\u{f071}');
            let detail_accent = if entry.is_error && !detail_is_guardrail {
                colors.error
            } else if detail_is_guardrail {
                colors.approval
            } else {
                colors.marker
            };

            let header = row![
                text(&entry.tool)
                    .size(colors.modal_title)
                    .color(detail_accent)
                    .font(mono)
                    .shaping(shaped),
                text(format!("  {}", entry.timestamp))
                    .size(colors.modal_text)
                    .color(colors.muted)
                    .font(mono)
                    .shaping(shaped),
            ];

            let summary = text(&entry.summary)
                .size(colors.modal_text)
                .color(detail_accent)
                .font(mono)
                .shaping(shaped);

            let separator = text("\u{2500}".repeat(40))
                .size(colors.modal_text * 0.8)
                .color(colors.muted)
                .font(mono)
                .shaping(shaped);

            let detail = text(&entry.detail)
                .size(colors.modal_text)
                .color(colors.muted)
                .font(mono)
                .shaping(shaped);

            let detail_col = column![header, summary, separator, detail].spacing(8);

            container(
                scrollable(detail_col)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .padding(16)
            .width(Length::FillPortion(3))
            .height(Length::Fill)
            .style(colors.detail_bg_style())
            .into()
        } else {
            container(
                text("Select an entry to view details")
                    .size(colors.modal_text)
                    .color(colors.muted)
                    .font(mono)
                    .shaping(shaped),
            )
            .center_x(Length::FillPortion(3))
            .center_y(Length::Fill)
            .style(colors.detail_bg_style())
            .into()
        };

        // --- Compose layout ---
        let body = row![left_panel, right_panel]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        let content = column![title_row, uuid_row, body]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(colors.modal_bg_style())
            .into()
    }
}
