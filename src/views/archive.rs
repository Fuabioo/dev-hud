use iced::widget::text::Shaping;
use iced::widget::{column, container, mouse_area, row, scrollable, space, text};
use iced::{mouse, Element, Length};

use crate::app::{Hud, Message};
use crate::session::*;
use crate::util;

impl Hud {
    pub(crate) fn view_archive_modal(&self, archive: &ArchiveModalState) -> Element<'_, Message> {
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

        // Collect archived session indices
        let archived_indices: Vec<usize> = claude
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.archived)
            .map(|(i, _)| i)
            .collect();

        // Title row
        let title = text(format!(
            "\u{f187} Archived Sessions ({} total)",
            archived_indices.len()
        ))
        .size(colors.modal_title)
        .color(colors.marker)
        .font(mono)
        .shaping(shaped);

        let close_btn = mouse_area(
            text("\u{f00d}")
                .size(colors.modal_title)
                .color(colors.marker)
                .font(mono)
                .shaping(shaped),
        )
        .on_press(Message::CloseArchiveModal)
        .interaction(mouse::Interaction::Pointer);

        let title_row = row![title, space::horizontal(), close_btn];

        // --- Left column: list of archived sessions ---
        let mut sessions_col = column![].spacing(4);

        for (list_idx, &session_idx) in archived_indices.iter().enumerate() {
            let session = &claude.sessions[session_idx];
            let is_selected = archive.selected_session == Some(list_idx);
            let is_hovered = !is_selected && archive.hovered_session == Some(list_idx);

            let fg = if is_selected {
                colors.marker
            } else if is_hovered {
                colors.hover_text
            } else {
                colors.muted
            };

            let slug = util::shorten_project(&session.project_slug);
            let id_snippet = if session.session_id.len() > 8 {
                &session.session_id[..8]
            } else {
                &session.session_id
            };

            let exit_label = if let Some(exited_at) = session.exited_at {
                match exited_at.duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => {
                        let total_secs = d.as_secs();
                        let hours = (total_secs / 3600) % 24;
                        let minutes = (total_secs / 60) % 60;
                        format!("exited {hours:02}:{minutes:02}")
                    }
                    Err(_) => "exited".to_string(),
                }
            } else {
                "archived".to_string()
            };

            let dim = if is_selected || is_hovered {
                fg
            } else {
                colors.muted
            };

            let session_row = row![
                text(format!("{slug} "))
                    .size(colors.modal_text)
                    .color(fg)
                    .font(mono)
                    .shaping(shaped),
                text(format!("{id_snippet}.. "))
                    .size(colors.modal_text * 0.85)
                    .color(dim)
                    .font(mono)
                    .shaping(shaped),
                text(exit_label)
                    .size(colors.modal_text * 0.85)
                    .color(dim)
                    .font(mono)
                    .shaping(shaped),
            ];

            let session_element: Element<'_, Message> = if is_selected {
                container(session_row)
                    .style(colors.selected_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else if is_hovered {
                container(session_row)
                    .style(colors.hover_style())
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            } else {
                container(session_row)
                    .padding(iced::Padding::ZERO.top(2).bottom(2))
                    .into()
            };

            sessions_col = sessions_col.push(
                mouse_area(session_element)
                    .on_press(Message::SelectArchivedSession(list_idx))
                    .on_enter(Message::HoverArchivedSession(list_idx))
                    .on_exit(Message::UnhoverArchivedSession(list_idx))
                    .interaction(mouse::Interaction::Pointer),
            );
        }

        let left_panel = scrollable(sessions_col)
            .width(Length::FillPortion(1))
            .height(Length::Fill);

        // --- Middle column: activity log for selected archived session ---
        let (middle_panel, right_panel): (Element<'_, Message>, Element<'_, Message>) =
            if let Some(list_idx) = archive.selected_session {
                if let Some(&session_idx) = archived_indices.get(list_idx) {
                    let entries = &claude.activity_logs[session_idx];

                    let mut entries_col = column![].spacing(2);
                    for (i, entry) in entries.iter().enumerate() {
                        let is_selected = archive.selected_entry == Some(i);
                        let is_hovered = !is_selected && archive.hovered_entry == Some(i);

                        let is_guardrail = entry.is_error && entry.summary.contains('\u{f071}');
                        let is_genuine_error = entry.is_error && !is_guardrail;

                        let accent = if is_genuine_error {
                            Some(colors.error)
                        } else if is_guardrail {
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

                        let icon_prefix = if is_genuine_error { "✘ " } else { "" };

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
                                .on_press(Message::SelectArchivedEntry(i))
                                .on_enter(Message::HoverArchivedEntry(i))
                                .on_exit(Message::UnhoverArchivedEntry(i))
                                .interaction(mouse::Interaction::Pointer),
                        );
                    }

                    let mid: Element<'_, Message> = scrollable(entries_col)
                        .width(Length::FillPortion(2))
                        .height(Length::Fill)
                        .into();

                    // Right panel: detail for selected entry
                    let right: Element<'_, Message> =
                        if let Some(entry_idx) = archive.selected_entry {
                            if let Some(entry) = entries.get(entry_idx) {
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

                                let detail_col =
                                    column![header, summary, separator, detail].spacing(8);

                                container(
                                    scrollable(detail_col)
                                        .width(Length::Fill)
                                        .height(Length::Fill),
                                )
                                .padding(16)
                                .width(Length::FillPortion(2))
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
                                .center_x(Length::FillPortion(2))
                                .center_y(Length::Fill)
                                .style(colors.detail_bg_style())
                                .into()
                            }
                        } else {
                            container(
                                text("Select an entry to view details")
                                    .size(colors.modal_text)
                                    .color(colors.muted)
                                    .font(mono)
                                    .shaping(shaped),
                            )
                            .center_x(Length::FillPortion(2))
                            .center_y(Length::Fill)
                            .style(colors.detail_bg_style())
                            .into()
                        };

                    (mid, right)
                } else {
                    let mid: Element<'_, Message> = container(
                        text("Session not found")
                            .size(colors.modal_text)
                            .color(colors.muted)
                            .font(mono)
                            .shaping(shaped),
                    )
                    .center_x(Length::FillPortion(2))
                    .center_y(Length::Fill)
                    .into();
                    let right: Element<'_, Message> = container(space::horizontal())
                        .width(Length::FillPortion(2))
                        .height(Length::Fill)
                        .style(colors.detail_bg_style())
                        .into();
                    (mid, right)
                }
            } else {
                let mid: Element<'_, Message> = container(
                    text("Select an archived session")
                        .size(colors.modal_text)
                        .color(colors.muted)
                        .font(mono)
                        .shaping(shaped),
                )
                .center_x(Length::FillPortion(2))
                .center_y(Length::Fill)
                .into();
                let right: Element<'_, Message> = container(space::horizontal())
                    .width(Length::FillPortion(2))
                    .height(Length::Fill)
                    .style(colors.detail_bg_style())
                    .into();
                (mid, right)
            };

        // --- Compose layout ---
        let body = row![left_panel, middle_panel, right_panel]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill);

        let content = column![title_row, body]
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
