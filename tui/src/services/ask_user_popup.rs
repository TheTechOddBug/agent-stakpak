//! Ask User Popup Component
//!
//! A full-width bottom popup that allows the LLM to ask the user structured questions
//! with predefined options and optional custom input.
//!
//! Design:
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────────────────┐
//! │  ← □ Visibility   □ Enrollment   ✓ Payment   □ Coach Access   Review →          │
//! ├──────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                  │
//! │  Should academies be public by default (visible to all users), or should they    │
//! │  require admin approval before being listed?                                     │
//! │                                                                                  │
//! │  [>] Public by default                                                           │
//! │       Academies are visible immediately after creation                            │
//! │  [2] Require approval                                                            │
//! │       Admin must approve before academy appears in listings                      │
//! │  [3] │Type your answer...                                                        │
//! │                                                                                  │
//! ├──────────────────────────────────────────────────────────────────────────────────┤
//! │  Enter select · ↑/↓ options · ←/→ questions · 1-9 quick select · Esc cancel     │
//! └──────────────────────────────────────────────────────────────────────────────────┘
//! ```

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use stakpak_shared::models::integrations::openai::AskUserQuestion;

/// Horizontal padding inside the content area (left side)
const CONTENT_PAD: &str = "  ";
const CONTENT_PAD_LEN: usize = 2;
/// Indent for option descriptions — aligns under label text after "[n] "
const DESC_INDENT: &str = "       "; // CONTENT_PAD + "[n] " + " "

/// Calculate the height needed for the ask user popup
pub fn calculate_ask_user_height(state: &AppState, terminal_width: u16) -> u16 {
    if !state.show_ask_user_popup || state.ask_user_questions.is_empty() {
        return 0;
    }

    // If we're on the Review tab, calculate based on question count
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        // Tab bar (1) + border (1) + pad (1)
        // + per question: label (1) + answer (1) + spacing (1) = 3 each
        // + warning or pad (1) + help (1) + border (1)
        let question_lines = state.ask_user_questions.len() * 3;
        let total = 1 + 1 + 1 + question_lines + 1 + 1 + 1;
        let max_height = (terminal_width as f32 * 0.6) as u16;
        return (total as u16).min(max_height).max(10);
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    // Available width for text = total - borders(2) - inner margin(2) - content padding(2)
    let inner_width = terminal_width.saturating_sub(4) as usize;
    let text_width = inner_width.saturating_sub(CONTENT_PAD_LEN);

    // Question text wrapped lines
    let question_lines = textwrap::wrap(&current_q.question, text_width).len();

    // Options: each option takes 1 line for label, optionally 1 for description
    let mut option_lines = 0;
    for opt in &current_q.options {
        option_lines += 1; // label line
        if opt.description.is_some() {
            option_lines += 1; // description line
        }
    }

    // Custom input option (if allowed): always 1 line (inline)
    let custom_lines = if current_q.allow_custom { 1 } else { 0 };

    // Height calculation:
    // - Tab bar: 1
    // - Top border: 1
    // - Pad line: 1
    // - Question text: question_lines
    // - Pad line: 1
    // - Options: option_lines
    // - Custom input: custom_lines
    // - Pad line: 1
    // - Help text: 1
    // - Bottom border: 1
    let total = 1 + 1 + 1 + question_lines + 1 + option_lines + custom_lines + 1 + 1 + 1;

    // Cap at 60% of terminal height
    let max_height = (terminal_width as f32 * 0.6) as u16;
    (total as u16).min(max_height).max(10) // minimum 10 lines
}

/// Render the ask user popup
pub fn render_ask_user_popup(f: &mut Frame, state: &AppState) {
    if !state.show_ask_user_popup || state.ask_user_questions.is_empty() {
        return;
    }

    let area = f.area();
    let popup_height = calculate_ask_user_height(state, area.width);

    // Position at bottom, full width with small margin
    let popup_area = Rect {
        x: area.x + 1,
        y: area.height.saturating_sub(popup_height),
        width: area.width.saturating_sub(2),
        height: popup_height,
    };

    // Clear background
    f.render_widget(Clear, popup_area);

    // Main border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(block, popup_area);

    let inner = popup_area.inner(Margin::new(1, 1));

    // Split: [tabs][content][help]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tabs
            Constraint::Min(3),    // content
            Constraint::Length(1), // help
        ])
        .split(inner);

    render_tab_bar(f, state, chunks[0]);

    // Check if we're on the Submit tab
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        render_submit_content(f, state, chunks[1]);
    } else {
        render_question_content(f, state, chunks[1]);
    }

    render_help_text(f, state, chunks[2]);
}

/// Render the tab bar showing all questions and Review
fn render_tab_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let mut spans = vec![];

    // Left padding + arrow
    spans.push(Span::styled(" ← ", Style::default().fg(Color::DarkGray)));

    for (i, q) in state.ask_user_questions.iter().enumerate() {
        let is_current = i == state.ask_user_current_tab;
        let is_answered = state.ask_user_answers.contains_key(&q.label);

        let checkbox = if is_answered { "✓ " } else { "□ " };

        let style = if is_current {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else if is_answered {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Truncate label if too long (char-safe for UTF-8)
        let label = if q.label.chars().count() > 15 {
            format!("{}...", q.label.chars().take(12).collect::<String>())
        } else {
            q.label.clone()
        };

        spans.push(Span::styled(format!("{}{}", checkbox, label), style));
        spans.push(Span::raw("   "));
    }

    // Submit tab
    let is_submit_tab = state.ask_user_current_tab >= state.ask_user_questions.len();
    let all_required_answered = check_all_required_answered(state);

    let submit_style = if is_submit_tab {
        Style::default().bg(Color::Cyan).fg(Color::Black)
    } else if all_required_answered {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    spans.push(Span::styled("Review", submit_style));

    // Right arrow
    spans.push(Span::styled(" →", Style::default().fg(Color::DarkGray)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render the question content area
fn render_question_content(f: &mut Frame, state: &AppState, area: Rect) {
    let q = &state.ask_user_questions[state.ask_user_current_tab];
    let mut lines = vec![];

    // Check if this question has a previous answer
    let previous_answer = state.ask_user_answers.get(&q.label);

    // Empty line for spacing
    lines.push(Line::from(""));

    // Question text (bold, wrapped — account for padding)
    let text_width = (area.width as usize).saturating_sub(CONTENT_PAD_LEN);
    let wrapped_question = textwrap::wrap(&q.question, text_width);
    for line in wrapped_question {
        lines.push(Line::from(vec![
            Span::raw(CONTENT_PAD),
            Span::styled(
                line.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Empty line
    lines.push(Line::from(""));

    // Options
    for (i, opt) in q.options.iter().enumerate() {
        let is_selected = i == state.ask_user_selected_option;
        let is_answered = previous_answer
            .map(|a| !a.is_custom && a.answer == opt.value)
            .unwrap_or(false);

        let (bracket, bracket_style) = if is_answered {
            (
                "[✓]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else if is_selected {
            (
                "[>]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (
                &*format!("[{}]", i + 1),
                Style::default().fg(Color::DarkGray),
            )
        };

        let label_style = if is_answered {
            Style::default().fg(Color::Cyan)
        } else if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let bracket_owned = bracket.to_string();
        lines.push(Line::from(vec![
            Span::raw(CONTENT_PAD),
            Span::styled(bracket_owned, bracket_style),
            Span::raw(" "),
            Span::styled(&opt.label, label_style),
        ]));

        if let Some(desc) = &opt.description {
            let desc_style = if is_selected || is_answered {
                Style::default().fg(Color::Gray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}", DESC_INDENT, desc),
                desc_style,
            )));
        }
    }

    // Custom input option (if allowed) — inline with placeholder
    if q.allow_custom {
        let custom_idx = q.options.len();
        let is_selected = state.ask_user_selected_option == custom_idx;
        let is_custom_answered = previous_answer.map(|a| a.is_custom).unwrap_or(false);

        let (bracket, bracket_style) = if is_custom_answered {
            (
                "[✓]".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else if is_selected {
            (
                "[>]".to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (
                format!("[{}]", custom_idx + 1),
                Style::default().fg(Color::DarkGray),
            )
        };

        if is_selected {
            // Active input: pad + bracket + cursor + typed text (or placeholder)
            if state.ask_user_custom_input.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw(CONTENT_PAD),
                    Span::styled(bracket, bracket_style),
                    Span::raw(" "),
                    Span::styled("│", Style::default().fg(Color::Cyan)),
                    Span::styled("Type your answer...", Style::default().fg(Color::DarkGray)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(CONTENT_PAD),
                    Span::styled(bracket, bracket_style),
                    Span::raw(" "),
                    Span::styled(
                        &state.ask_user_custom_input,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("│", Style::default().fg(Color::Cyan)),
                ]));
            }
        } else if is_custom_answered && let Some(answer) = previous_answer {
            lines.push(Line::from(vec![
                Span::raw(CONTENT_PAD),
                Span::styled(bracket, bracket_style),
                Span::raw(" "),
                Span::styled(&answer.answer, Style::default().fg(Color::Cyan)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw(CONTENT_PAD),
                Span::styled(bracket, bracket_style),
                Span::raw(" "),
                Span::styled("Other...", Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// Render the submit tab content — full Q&A summary
fn render_submit_content(f: &mut Frame, state: &AppState, area: Rect) {
    let mut lines = vec![];
    let all_required_answered = check_all_required_answered(state);
    let inner_width = (area.width as usize).saturating_sub(CONTENT_PAD_LEN);

    lines.push(Line::from(""));

    // Show each question with its answer (or missing marker)
    for q in &state.ask_user_questions {
        // Question label
        let label_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let required_marker = if q.required { " *" } else { "" };
        lines.push(Line::from(vec![
            Span::raw(CONTENT_PAD),
            Span::styled(&q.label, label_style),
            Span::styled(required_marker, Style::default().fg(Color::Red)),
        ]));

        // Answer or missing
        if let Some(answer) = state.ask_user_answers.get(&q.label) {
            // Find the display label for the selected option
            let display = if answer.is_custom {
                answer.answer.clone()
            } else {
                q.options
                    .iter()
                    .find(|o| o.value == answer.answer)
                    .map(|o| o.label.clone())
                    .unwrap_or_else(|| answer.answer.clone())
            };

            // Truncate to available width (char-safe)
            let max_display = inner_width.saturating_sub(CONTENT_PAD_LEN);
            let display = if display.chars().count() > max_display {
                format!(
                    "{}…",
                    display
                        .chars()
                        .take(max_display.saturating_sub(1))
                        .collect::<String>()
                )
            } else {
                display
            };

            lines.push(Line::from(vec![
                Span::raw(CONTENT_PAD),
                Span::raw(CONTENT_PAD),
                Span::styled(display, Style::default().fg(Color::Cyan)),
            ]));
        } else if q.required {
            lines.push(Line::from(vec![
                Span::raw(CONTENT_PAD),
                Span::styled("  □ ", Style::default().fg(Color::Yellow)),
                Span::styled("not answered", Style::default().fg(Color::Yellow)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw(CONTENT_PAD),
                Span::styled("  — ", Style::default().fg(Color::DarkGray)),
                Span::styled("skipped", Style::default().fg(Color::DarkGray)),
            ]));
        }

        // Spacing between questions
        lines.push(Line::from(""));
    }

    // Show warning if not all required questions are answered
    if !all_required_answered {
        lines.push(Line::from(vec![
            Span::raw(CONTENT_PAD),
            Span::styled(
                "Answer all required (*) questions to submit",
                Style::default().fg(Color::Yellow),
            ),
        ]));
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// Render the help text at the bottom
fn render_help_text(f: &mut Frame, state: &AppState, area: Rect) {
    let is_submit_tab = state.ask_user_current_tab >= state.ask_user_questions.len();
    let all_required_answered = check_all_required_answered(state);

    let help = if is_submit_tab && all_required_answered {
        Line::from(vec![
            Span::raw(CONTENT_PAD),
            Span::styled("Enter", Style::default().fg(Color::DarkGray)),
            Span::styled(" submit", Style::default().fg(Color::Green)),
            Span::raw(" · "),
            Span::styled("←/→", Style::default().fg(Color::DarkGray)),
            Span::styled(" questions", Style::default().fg(Color::Cyan)),
            Span::raw(" · "),
            Span::styled("Esc", Style::default().fg(Color::DarkGray)),
            Span::styled(" cancel", Style::default().fg(Color::Cyan)),
        ])
    } else if is_submit_tab {
        Line::from(vec![
            Span::raw(CONTENT_PAD),
            Span::styled("←/→", Style::default().fg(Color::DarkGray)),
            Span::styled(" questions", Style::default().fg(Color::Cyan)),
            Span::raw(" · "),
            Span::styled("Esc", Style::default().fg(Color::DarkGray)),
            Span::styled(" cancel", Style::default().fg(Color::Cyan)),
        ])
    } else {
        // Check if custom input is selected
        let current_q = &state.ask_user_questions[state.ask_user_current_tab];
        let is_custom_selected =
            current_q.allow_custom && state.ask_user_selected_option == current_q.options.len();

        if is_custom_selected {
            Line::from(vec![
                Span::raw(CONTENT_PAD),
                Span::styled("Type", Style::default().fg(Color::DarkGray)),
                Span::styled(" your answer", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Enter", Style::default().fg(Color::DarkGray)),
                Span::styled(" confirm", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("↑/↓", Style::default().fg(Color::DarkGray)),
                Span::styled(" options", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Esc", Style::default().fg(Color::DarkGray)),
                Span::styled(" cancel", Style::default().fg(Color::Cyan)),
            ])
        } else {
            Line::from(vec![
                Span::raw(CONTENT_PAD),
                Span::styled("Enter", Style::default().fg(Color::DarkGray)),
                Span::styled(" select", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("↑/↓", Style::default().fg(Color::DarkGray)),
                Span::styled(" options", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("←/→", Style::default().fg(Color::DarkGray)),
                Span::styled(" questions", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("1-9", Style::default().fg(Color::DarkGray)),
                Span::styled(" quick select", Style::default().fg(Color::Cyan)),
                Span::raw(" · "),
                Span::styled("Esc", Style::default().fg(Color::DarkGray)),
                Span::styled(" cancel", Style::default().fg(Color::Cyan)),
            ])
        }
    };

    f.render_widget(Paragraph::new(help), area);
}

/// Check if all required questions have been answered
fn check_all_required_answered(state: &AppState) -> bool {
    state
        .ask_user_questions
        .iter()
        .filter(|q| q.required)
        .all(|q| state.ask_user_answers.contains_key(&q.label))
}

/// Get the total number of options for the current question (including custom if allowed)
pub fn get_total_options(question: &AskUserQuestion) -> usize {
    if question.allow_custom {
        question.options.len() + 1
    } else {
        question.options.len()
    }
}
