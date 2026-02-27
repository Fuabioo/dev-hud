use iced::widget::text::Shaping;
use iced::widget::{
    column, container, image as iced_image, mouse_area, row, space, svg, text,
};
use iced::{mouse, Element, Length};

use crate::app::{Hud, HudMode, Message, EDGE_MARGIN};
use crate::loader::*;
use crate::session::*;
use crate::shell;
use crate::util;
use crate::util::truncate_str;

impl Hud {
    pub(crate) fn view_hud(&self) -> Element<'_, Message> {
        let mono = self.current_font();
        let shaped = Shaping::Advanced;
        let colors = &self.colors;
        let marker = || text("+").size(colors.marker_size).color(colors.marker);

        // Top row: corner markers only
        let top_row = row![marker(), space::horizontal(), marker()];

        // Build bottom row (with optional loader widget)
        let bottom_row = if let Some(loader) = &self.demo_loader {
            let label: Element<'_, Message> = text(format!(" {}", loader.style.label()))
                .size(colors.info_text)
                .color(colors.muted)
                .into();

            let widget: Element<'_, Message> = match loader.style {
                LoaderStyle::Braille | LoaderStyle::Bounce | LoaderStyle::Pipe => {
                    let frames = loader.style.text_frames();
                    let ch = frames[loader.frame % frames.len()];
                    text(format!(" {ch}"))
                        .size(colors.label_text)
                        .color(colors.marker)
                        .font(mono)
                        .shaping(shaped)
                        .into()
                }
                LoaderStyle::Gif => {
                    if loader.gif_frames.is_empty() {
                        text(" ?").size(colors.label_text).color(colors.marker).into()
                    } else {
                        let handle =
                            loader.gif_frames[loader.frame % loader.gif_frames.len()].clone();
                        container(
                            iced_image(handle)
                                .width(LOADER_IMAGE_SIZE)
                                .height(LOADER_IMAGE_SIZE),
                        )
                        .padding(iced::padding::left(4))
                        .into()
                    }
                }
                LoaderStyle::Svg => {
                    if loader.svg_frames.is_empty() {
                        text(" ?").size(colors.label_text).color(colors.marker).into()
                    } else {
                        let handle =
                            loader.svg_frames[loader.frame % loader.svg_frames.len()].clone();
                        container(svg(handle).width(LOADER_IMAGE_SIZE).height(LOADER_IMAGE_SIZE))
                            .padding(iced::padding::left(4))
                            .into()
                    }
                }
            };

            row![marker(), widget, label, space::horizontal(), marker()]
        } else {
            row![marker(), space::horizontal(), marker()]
        };

        // Build main column
        let mut main_col = column![top_row]
            .width(Length::Fill)
            .height(Length::Fill);

        // Build left widget (claude sessions) and right widget (shells) independently,
        // then combine them in a row so they don't affect each other's vertical position.
        let claude_widget: Element<'_, Message> = if let Some(claude) = self.active_claude() {
            let focused = self.mode == HudMode::Focused;
            let max_chars: usize = if focused { 512 } else { 64 };

            // Collect non-archived session indices (preserves original Vec indices for modal)
            let visible: Vec<usize> = claude
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| !s.archived)
                .map(|(i, _)| i)
                .collect();
            let show_from = visible.len().saturating_sub(MAX_VISIBLE_SESSIONS);
            let display = &visible[show_from..];

            let mut session_col = column![];

            for &i in display {
                let session = &claude.sessions[i];

                // Dim sessions in grace period (exited but not yet archived)
                let in_grace_period = session.exited_at.is_some() && !session.archived;

                let (icon_str, attention_color) = if session.exited_at.is_some() {
                    ("\u{f04d}", None) // nf-fa-stop
                } else if session.needs_attention {
                    ("\u{f0f3}", Some(colors.approval)) // nf-fa-bell, orange
                } else {
                    let icon = match &session.current_tool {
                        Some(tool) if session.active => {
                            let frames = tool_state_frames(tool.category);
                            frames[(claude.spinner_frame / 4) % frames.len()]
                        }
                        _ => {
                            if session.active {
                                claude.spinner_char()
                            } else {
                                session.kind.icon(focused)
                            }
                        }
                    };
                    (icon, None)
                };

                let is_hovered = focused && self.hovered_session == Some(i);
                let fg = if is_hovered {
                    colors.hover_text
                } else if let Some(ac) = attention_color {
                    ac
                } else if in_grace_period {
                    colors.muted
                } else {
                    colors.marker
                };
                let dim = if is_hovered {
                    colors.hover_text
                } else {
                    colors.muted
                };

                let activity = truncate_str(&session.activity, max_chars);

                let mut srow = row![];

                srow = srow.push(
                    text(format!("{icon_str} "))
                        .size(colors.widget_text)
                        .color(fg)
                        .font(mono)
                        .shaping(shaped),
                );

                // Show project slug prefix in focused mode always,
                // and in non-focus mode for exited sessions (static text won't clip badly)
                if focused || session.exited_at.is_some() {
                    let slug = util::shorten_project(&session.project_slug);
                    srow = srow.push(
                        text(format!("{slug} "))
                            .size(colors.widget_text)
                            .color(dim)
                            .font(mono)
                            .shaping(shaped),
                    );
                }

                srow = srow.push(
                    text(activity)
                        .size(colors.widget_text)
                        .color(fg)
                        .font(mono)
                        .shaping(shaped),
                );

                let session_element: Element<'_, Message> = srow.into();
                if focused {
                    let wrapped: Element<'_, Message> = if is_hovered {
                        container(session_element)
                            .style(colors.hover_style())
                            .into()
                    } else {
                        session_element
                    };
                    session_col = session_col.push(
                        mouse_area(wrapped)
                            .on_press(Message::OpenSessionModal(i))
                            .on_enter(Message::HoverSession(i))
                            .on_exit(Message::UnhoverSession(i))
                            .interaction(mouse::Interaction::Pointer),
                    );
                } else {
                    session_col = session_col.push(session_element);
                }

                // --- Subagent tree rendering ---
                if !session.subagents.is_empty() {
                    let max_sub = if focused { 5 } else { 2 };

                    // Collect which subagents to display:
                    // In non-focused mode, only active + needs_attention.
                    // In focused mode, show all (up to max).
                    let mut display_subs: Vec<usize> = if focused {
                        (0..session.subagents.len()).collect()
                    } else {
                        session
                            .subagents
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| s.active || s.needs_attention)
                            .map(|(j, _)| j)
                            .collect()
                    };

                    let overflow = display_subs.len().saturating_sub(max_sub);
                    display_subs.truncate(max_sub);

                    for (pos, &sub_idx) in display_subs.iter().enumerate() {
                        let sub = &session.subagents[sub_idx];
                        let is_last = pos == display_subs.len() - 1 && overflow == 0;
                        let prefix = if is_last { "  └─ " } else { "  ├─ " };

                        let sub_icon = if sub.needs_attention {
                            "\u{f0f3}" // bell
                        } else if sub.current_tool.is_some() {
                            let frames = match &sub.current_tool {
                                Some(t) => tool_state_frames(t.category),
                                None => LoaderStyle::Braille.text_frames(),
                            };
                            frames[(claude.spinner_frame / 4) % frames.len()]
                        } else if sub.active {
                            claude.spinner_char()
                        } else {
                            "\u{f00c}" // checkmark
                        };

                        let (sub_icon_color, sub_text_color) = if sub.needs_attention {
                            (colors.approval, colors.approval)
                        } else if sub.active {
                            (colors.marker, colors.marker)
                        } else {
                            (colors.muted, colors.muted)
                        };

                        let sub_desc = if sub.description.is_empty() {
                            &sub.activity
                        } else {
                            &sub.description
                        };
                        let sub_text = if sub.active && !sub.activity.is_empty() {
                            format!("{}: {}", truncate_str(sub_desc, 30), truncate_str(&sub.activity, max_chars.saturating_sub(34)))
                        } else {
                            truncate_str(sub_desc, max_chars)
                        };

                        let sub_row = row![
                            text(format!("{prefix}{sub_icon} "))
                                .size(colors.widget_text)
                                .color(sub_icon_color)
                                .font(mono)
                                .shaping(shaped),
                            text(sub_text)
                                .size(colors.widget_text)
                                .color(sub_text_color)
                                .font(mono)
                                .shaping(shaped),
                        ];
                        session_col = session_col.push(sub_row);
                    }

                    if overflow > 0 {
                        let more_row = row![text(format!("  └─ +{overflow} more"))
                            .size(colors.widget_text)
                            .color(colors.muted)
                            .font(mono)
                            .shaping(shaped)];
                        session_col = session_col.push(more_row);
                    }
                }
            }

            // Archive pill: show count of archived sessions
            let archived_count = claude.sessions.iter().filter(|s| s.archived).count();
            if archived_count > 0 && focused {
                let pill_fg = if self.hovered_archive {
                    colors.hover_text
                } else {
                    colors.muted
                };
                let pill_text = text(format!(" Archived ({archived_count})"))
                    .size(colors.widget_text * 0.9)
                    .color(pill_fg)
                    .font(mono)
                    .shaping(shaped);
                let pill_element: Element<'_, Message> = if self.hovered_archive {
                    container(pill_text).style(colors.hover_style()).into()
                } else {
                    pill_text.into()
                };
                session_col = session_col.push(
                    mouse_area(pill_element)
                        .on_press(Message::OpenArchiveModal)
                        .on_enter(Message::HoverArchive)
                        .on_exit(Message::UnhoverArchive)
                        .interaction(mouse::Interaction::Pointer),
                );
            }

            if self.backdrop {
                container(session_col)
                    .style(colors.hud_backdrop_style())
                    .padding(6)
                    .into()
            } else {
                session_col.into()
            }
        } else {
            space::Space::new().height(0).width(0).into()
        };

        // --- Shell widgets: render instances grouped by position ---
        //
        // Macro to render a single shell instance's content into a column.
        // Uses a macro instead of a closure to avoid lifetime issues with
        // iced's Column type (which doesn't implement Default).
        macro_rules! render_shell_inst {
            ($col:expr, $inst:expr, $full:expr) => {{
                let inst = $inst;
                let full: bool = $full;
                let inst_font_size = inst.config.font_size.unwrap_or(colors.widget_text);
                let inst_cols = inst.config.cols;
                let icon = "\u{f120}";

                let label_row = row![
                    text(format!("{icon} "))
                        .size(inst_font_size)
                        .color(colors.muted)
                        .font(mono)
                        .shaping(shaped),
                    text(&inst.config.label)
                        .size(inst_font_size)
                        .color(colors.muted)
                        .font(mono)
                        .shaping(shaped),
                ];
                $col = $col.push(label_row);

                if inst.resolved_mode == shell::ShellMode::Tui {
                    if let Some(ref screen) = inst.tui_screen {
                        for row_str in screen {
                            let truncated = truncate_str(row_str, inst_cols);
                            let out_line = row![text(format!("  {truncated}"))
                                .size(inst_font_size)
                                .color(colors.marker)
                                .font(mono)
                                .shaping(shaped)];
                            $col = $col.push(out_line);
                        }
                    } else if full {
                        $col = $col.push(row![text("  ...")
                            .size(inst_font_size)
                            .color(colors.muted)
                            .font(mono)
                            .shaping(shaped)]);
                    }
                    if full {
                        if let Some(code) = inst.exit_code {
                            $col = $col.push(row![text(format!("  exit {code}"))
                                .size(inst_font_size)
                                .color(colors.muted)
                                .font(mono)
                                .shaping(shaped)]);
                        }
                    }
                } else if let Some(ref err) = inst.error {
                    $col = $col.push(row![text(format!(
                        "  \u{f071} {}",
                        truncate_str(err, inst_cols.saturating_sub(4))
                    ))
                    .size(inst_font_size)
                    .color(colors.error)
                    .font(mono)
                    .shaping(shaped)]);
                } else if inst.buffer.is_empty() {
                    if full {
                        if let Some(code) = inst.exit_code {
                            $col = $col.push(row![text(format!("  exit {code}"))
                                .size(inst_font_size)
                                .color(colors.muted)
                                .font(mono)
                                .shaping(shaped)]);
                        } else {
                            $col = $col.push(row![text("  ...")
                                .size(inst_font_size)
                                .color(colors.muted)
                                .font(mono)
                                .shaping(shaped)]);
                        }
                    }
                } else {
                    let visible_lines = inst.config.lines;
                    let start = inst.buffer.len().saturating_sub(visible_lines);
                    for line in inst.buffer.iter().skip(start) {
                        let truncated = truncate_str(line, inst_cols);
                        $col = $col.push(row![text(format!("  {truncated}"))
                            .size(inst_font_size)
                            .color(colors.marker)
                            .font(mono)
                            .shaping(shaped)]);
                    }
                    if full {
                        if let Some(code) = inst.exit_code {
                            $col = $col.push(row![text(format!("  exit {code}"))
                                .size(inst_font_size)
                                .color(colors.muted)
                                .font(mono)
                                .shaping(shaped)]);
                        }
                    }
                }
            }};
        }

        // Build a shell widget Element for a given screen position.
        // In focused mode all instances at that position render fully.
        // In unfocused mode only `visible: always` instances render (plus
        // a single most-recent line for non-always widgets in bottom-right).
        let focused = self.mode == HudMode::Focused;

        macro_rules! build_position_widget {
            ($pos:expr) => {{
                let pos = $pos;
                let widget: Element<'_, Message> = if let Some(shells) = &self.shells {
                    let mut col = column![];
                    let mut has_content = false;

                    for inst in &shells.instances {
                        if inst.config.position != pos {
                            continue;
                        }
                        if focused {
                            render_shell_inst!(col, inst, true);
                            has_content = true;
                        } else if inst.config.visible == shell::Visibility::Always {
                            render_shell_inst!(col, inst, false);
                            has_content = true;
                        }
                    }

                    // In unfocused mode, show single most-recent line for non-always
                    // widgets that belong to this position
                    if !focused && pos == shell::Position::BottomRight {
                        if let Some(idx) = shells.most_recent {
                            if let Some(inst) = shells.instances.get(idx) {
                                if inst.config.visible != shell::Visibility::Always
                                    && inst.config.position == pos
                                {
                                    let icon = "\u{f120}";
                                    let inst_cols = inst.config.cols;
                                    let last_line = inst
                                        .buffer
                                        .back()
                                        .map(|l| truncate_str(l, inst_cols))
                                        .or_else(|| {
                                            inst.error
                                                .as_ref()
                                                .map(|e| truncate_str(e, inst_cols))
                                        })
                                        .unwrap_or_default();

                                    let shell_row = row![
                                        text(format!("{icon} "))
                                            .size(colors.widget_text)
                                            .color(colors.muted)
                                            .font(mono)
                                            .shaping(shaped),
                                        text(format!("{} ", inst.config.label))
                                            .size(colors.widget_text)
                                            .color(colors.muted)
                                            .font(mono)
                                            .shaping(shaped),
                                        text(last_line)
                                            .size(colors.widget_text)
                                            .color(colors.marker)
                                            .font(mono)
                                            .shaping(shaped),
                                    ];
                                    col = col.push(shell_row);
                                    has_content = true;
                                }
                            }
                        }
                    }

                    if has_content {
                        if self.backdrop && focused {
                            container(col)
                                .style(colors.hud_backdrop_style())
                                .padding(6)
                                .into()
                        } else {
                            col.into()
                        }
                    } else {
                        space::Space::new().height(0).width(0).into()
                    }
                } else {
                    space::Space::new().height(0).width(0).into()
                };
                widget
            }};
        }

        let shell_top_left = build_position_widget!(shell::Position::TopLeft);
        let shell_top_right = build_position_widget!(shell::Position::TopRight);
        let shell_bottom_left = build_position_widget!(shell::Position::BottomLeft);
        let shell_bottom_right = build_position_widget!(shell::Position::BottomRight);

        // Top widgets row: top-left shells + space + top-right shells
        let top_widgets_row = row![
            shell_top_left,
            space::horizontal(),
            shell_top_right,
        ]
        .width(Length::Fill)
        .align_y(iced::alignment::Vertical::Top);

        main_col = main_col.push(top_widgets_row);
        main_col = main_col.push(space::vertical());

        // Bottom widgets row: claude + bottom-left shells (left) + space + bottom-right shells
        let left_bottom: Element<'_, Message> = {
            let mut left_col = column![];
            left_col = left_col.push(claude_widget);
            left_col = left_col.push(shell_bottom_left);
            left_col.into()
        };

        let widgets_row = row![
            left_bottom,
            space::horizontal(),
            shell_bottom_right,
        ]
        .width(Length::Fill)
        .align_y(iced::alignment::Vertical::Bottom);

        main_col = main_col.push(widgets_row);

        main_col = main_col.push(bottom_row);

        // Info line: version, commit, font — below the marker rectangle
        let info_size = colors.info_text;
        let info_row = row![
            space::horizontal(),
            text(format!(
                "v{} {} {}",
                env!("DEV_HUD_VERSION"),
                env!("DEV_HUD_COMMIT"),
                self.current_font_label()
            ))
            .size(info_size)
            .color(colors.muted)
            .font(mono)
            .shaping(shaped)
        ];

        let outer = column![
            container(main_col)
                .padding(EDGE_MARGIN)
                .width(Length::Fill)
                .height(Length::Fill),
            container(info_row)
                .padding(iced::Padding {
                    top: 0.0,
                    right: EDGE_MARGIN as f32,
                    bottom: 8.0,
                    left: 0.0,
                })
                .width(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        outer.into()
    }
}
