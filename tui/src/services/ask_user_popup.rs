//! Ask User Popup Component
//!
//! A full-width bottom popup that allows the LLM to ask the user structured questions
//! with predefined options and optional custom input.
//!
//! Design:
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────────┐
//! │ ← □ Visibility   □ Enrollment   ✓ Payment   □ Coach Access   ✓ Submit →         │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                 │
//! │ Should academies be public by default (visible to all users), or should they   │
//! │ require admin approval before being listed?                                     │
//! │                                                                                 │
//! │ › 1. Public by default                                                          │
//! │      Academies are visible immediately after creation                           │
//! │                                                                                 │
//! │   2. Require approval                                                           │
//! │      Admin must approve before academy appears in listings                      │
//! │                                                                                 │
//! │   3. Type something...                                                          │
//! │      [Custom input field when selected]                                         │
//! │                                                                                 │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │ Enter to select · Tab/Arrow keys to navigate · Esc to cancel                    │
//! └─────────────────────────────────────────────────────────────────────────────────┘
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

/// Calculate the height needed for the ask user popup
pub fn calculate_ask_user_height(state: &AppState, terminal_width: u16) -> u16 {
    if !state.show_ask_user_popup || state.ask_user_questions.is_empty() {
        return 0;
    }

    // If we're on the Submit tab, show a minimal height
    if state.ask_user_current_tab >= state.ask_user_questions.len() {
        // Tab bar (1) + border (1) + submit message (3) + help (1) + border (1)
        return 7;
    }

    let current_q = &state.ask_user_questions[state.ask_user_current_tab];
    let inner_width = terminal_width.saturating_sub(4) as usize; // borders + padding

    // Question text wrapped lines
    let question_lines = textwrap::wrap(&current_q.question, inner_width).len();

    // Options: each option takes 1 line for label, optionally 1 for description
    let mut option_lines = 0;
    for opt in &current_q.options {
        option_lines += 1; // label line
        if opt.description.is_some() {
            option_lines += 1; // description line
        }
    }

    // Custom input option (if allowed): 1 for "Type something...", 1 for input field when selected
    let custom_lines = if current_q.allow_custom {
        let custom_idx = current_q.options.len();
        if state.ask_user_selected_option == custom_idx {
            2 // "Type something..." + input field
        } else {
            1 // just "Type something..."
        }
    } else {
        0
    };

    // Height calculation:
    // - Tab bar: 1
    // - Top border: 1
    // - Empty line: 1
    // - Question text: question_lines
    // - Empty line: 1
    // - Options: option_lines
    // - Custom input: custom_lines
    // - Empty line: 1
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

/// Render the tab bar showing all questions and Submit
fn render_tab_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let mut spans = vec![];

    // Left arrow
    spans.push(Span::styled("← ", Style::default().fg(Color::DarkGray)));

    for (i, q) in state.ask_user_questions.iter().enumerate() {
        let is_current = i == state.ask_user_current_tab;
        let is_answered = state.ask_user_answers.contains_key(&q.id);

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

    spans.push(Span::styled("✓ Submit", submit_style));

    // Right arrow
    spans.push(Span::styled(" →", Style::default().fg(Color::DarkGray)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render the question content area
fn render_question_content(f: &mut Frame, state: &AppState, area: Rect) {
    let q = &state.ask_user_questions[state.ask_user_current_tab];
    let mut lines = vec![];

    // Check if this question has a previous answer
    let previous_answer = state.ask_user_answers.get(&q.id);

    // Empty line for spacing
    lines.push(Line::from(""));

    // Question text (bold, wrapped)
    let wrapped_question = textwrap::wrap(&q.question, area.width as usize);
    for line in wrapped_question {
        lines.push(Line::from(Span::styled(
            line.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Empty line
    lines.push(Line::from(""));

    // Options
    for (i, opt) in q.options.iter().enumerate() {
        let is_selected = i == state.ask_user_selected_option;
        let is_answered = previous_answer
            .map(|a| !a.is_custom && a.answer == opt.value)
            .unwrap_or(false);

        let prefix = if is_selected {
            "› "
        } else if is_answered {
            "✓ "
        } else {
            "  "
        };
        let num = format!("{}. ", i + 1);

        let style = if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if is_answered {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(num, style),
            Span::styled(&opt.label, style),
        ]));

        if let Some(desc) = &opt.description {
            let desc_style = if is_selected {
                Style::default().fg(Color::Gray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(Span::styled(
                format!("     {}", desc),
                desc_style,
            )));
        }
    }

    // Custom input option (if allowed)
    if q.allow_custom {
        let custom_idx = q.options.len();
        let is_selected = state.ask_user_selected_option == custom_idx;
        let is_custom_answered = previous_answer.map(|a| a.is_custom).unwrap_or(false);

        let prefix = if is_selected {
            "› "
        } else if is_custom_answered {
            "✓ "
        } else {
            "  "
        };
        let num = format!("{}. ", custom_idx + 1);

        let style = if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if is_custom_answered {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(num, style),
            Span::styled("Type something...", style),
        ]));

        // Show input field when custom option is selected OR show previous custom answer
        if is_selected {
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    &state.ask_user_custom_input,
                    Style::default().fg(Color::White),
                ),
                Span::styled("│", Style::default().fg(Color::Cyan)),
            ]));
        } else if is_custom_answered && let Some(answer) = previous_answer {
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(&answer.answer, Style::default().fg(Color::Cyan)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// Render the submit tab content
fn render_submit_content(f: &mut Frame, state: &AppState, area: Rect) {
    let mut lines = vec![];

    lines.push(Line::from(""));

    let all_required_answered = check_all_required_answered(state);

    if all_required_answered {
        lines.push(Line::from(Span::styled(
            "Ready to submit your answers!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press Enter to confirm and send your responses.",
            Style::default().fg(Color::White),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Some required questions are not answered yet.",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // List unanswered required questions
        for q in &state.ask_user_questions {
            if q.required && !state.ask_user_answers.contains_key(&q.id) {
                lines.push(Line::from(vec![
                    Span::styled("  □ ", Style::default().fg(Color::Yellow)),
                    Span::styled(&q.label, Style::default().fg(Color::White)),
                ]));
            }
        }
    }

    // Summary of answers
    if !state.ask_user_answers.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Your answers:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        for q in &state.ask_user_questions {
            if let Some(answer) = state.ask_user_answers.get(&q.id) {
                // Truncate answer display (char-safe for UTF-8)
                let answer_display = if answer.answer.chars().count() > 40 {
                    format!("{}...", answer.answer.chars().take(37).collect::<String>())
                } else {
                    answer.answer.clone()
                };
                lines.push(Line::from(vec![
                    Span::styled("  ✓ ", Style::default().fg(Color::Green)),
                    Span::styled(format!("{}: ", q.label), Style::default().fg(Color::White)),
                    Span::styled(
                        answer_display,
                        Style::default().fg(if answer.is_custom {
                            Color::Magenta
                        } else {
                            Color::Cyan
                        }),
                    ),
                ]));
            }
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// Render the help text at the bottom
fn render_help_text(f: &mut Frame, state: &AppState, area: Rect) {
    let is_submit_tab = state.ask_user_current_tab >= state.ask_user_questions.len();
    let all_required_answered = check_all_required_answered(state);

    let help = if is_submit_tab && all_required_answered {
        Line::from(vec![
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
        .all(|q| state.ask_user_answers.contains_key(&q.id))
}

/// Get the total number of options for the current question (including custom if allowed)
pub fn get_total_options(question: &AskUserQuestion) -> usize {
    if question.allow_custom {
        question.options.len() + 1
    } else {
        question.options.len()
    }
}
