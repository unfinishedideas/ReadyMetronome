/// This file controls the ratatui user interface display. It conditionally renders different screens based on the state
/// defined in App.rs
/// This is loosely based on the JSON Editor tutorial for ratatui. Tutorial found here https://ratatui.rs/tutorials/json-editor/ui/
use crate::app::{App, CurrentScreen, CurrentlyEditing};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

// This is the function to render the UI to the screen
pub fn ui(f: &mut Frame, app: &mut App) {
    // pop up block to use for editing / quit dialog
    let popup_block = Block::default()
        .title("Editing Value")
        .borders(Borders::NONE)
        .style(Style::default().bg(Color::Black));
    let area = centered_rect(50, 50, f.size());

    // various text styles for different situations
    let active_style = Style::default().bg(Color::LightYellow).fg(Color::Black);
    let quit_style = Style::default().fg(Color::Red);
    let error_style = Style::default().fg(Color::Red);

    // this defines the overall layout into three sections with the middle one being resizeable
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(f.size());

    // Title bar -------------------------------------------------------------------------------------------------------
    let title_block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default());

    let title = Paragraph::new(Text::styled(
        "Ready Metronome",
        Style::default().fg(Color::Green),
    ))
    .block(title_block);

    f.render_widget(title, chunks[0]);

    // Main screen -----------------------------------------------------------------------------------------------------
    // for the main menu screen we will use a widgets::List and ListState which we define from items in main.rs
    // loading in vector of items from main_menu and edit_menu for rendering
    let main_items: Vec<ListItem> = app
        .main_menu
        .items
        .iter()
        .map(|i| ListItem::new(i.as_str()))
        .collect();
    let main_list = List::new(main_items)
        .block(
            Block::default()
                .title("Control Panel")
                .borders(Borders::ALL),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(active_style);

    // define the main page layout and render (between the header and footer bars)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(chunks[1]);

    f.render_stateful_widget(main_list, main_chunks[0], &mut app.main_menu.state);

    // Right Panel -----------------------------------------------------------------------------------------------------
    let right_panel_items: Vec<ListItem> = if app.current_screen != CurrentScreen::SoundSelection {
        app.edit_menu
            .items
            .iter()
            .map(|i| ListItem::new(i.as_str()))
            .collect()
    } else {
        app.sound_selection_menu
            .items
            .iter()
            .map(|i| ListItem::new(i.as_str()))
            .collect()
    };
    let right_panel_list = List::new(right_panel_items)
        .block(
            Block::default()
                .title(if app.current_screen == CurrentScreen::SoundSelection {
                    "Sound Selection"
                } else {
                    "Status"
                })
                .borders(Borders::ALL),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(active_style);

    if app.current_screen == CurrentScreen::SoundSelection {
        f.render_stateful_widget(
            right_panel_list,
            main_chunks[1],
            &mut app.sound_selection_menu.state,
        );
    } else {
        f.render_stateful_widget(right_panel_list, main_chunks[1], &mut app.edit_menu.state);
    }

    // Editing Value Pop Up --------------------------------------------------------------------------------------------
    if let Some(editing) = app.currently_editing {
        f.render_widget(Clear, f.size()); //this clears the entire screen and anything already drawn
        f.render_widget(popup_block, area);

        // here we create two layouts, one to split the pop up vertically into two slices, and another to split the top
        // slice into two slices to hold the original value and the new value currently being edited
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let sub_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[0]);

        // get variables ready for conditional assignment
        let original_block;
        let key_block;
        let original_text;

        // the alert block is always the same
        let alert_block = Block::default().title("Notification").borders(Borders::ALL);
        let alert_text = Paragraph::new(app.alert_string.clone().red()).block(alert_block);

        match editing {
            CurrentlyEditing::Volume => {
                key_block = Block::default()
                    .title("Enter New Volume")
                    .borders(Borders::ALL);
                original_block = Block::default()
                    .title("Current Volume")
                    .borders(Borders::ALL);
                original_text = Paragraph::new(app.get_volume().to_string()).block(original_block);
            }
            CurrentlyEditing::Bpm => {
                key_block = Block::default()
                    .title("Enter New Bpm")
                    .borders(Borders::ALL);
                original_block = Block::default().title("Current Bpm").borders(Borders::ALL);
                original_text = Paragraph::new(app.get_bpm().to_string()).block(original_block);
            }
            CurrentlyEditing::TimeSignature => {
                key_block = Block::default()
                    .title("Enter new Time Signature (match format!)")
                    .borders(Borders::ALL);
                original_block = Block::default()
                    .title("Current Time Signature")
                    .borders(Borders::ALL);
                original_text =
                    Paragraph::new(app.get_time_sig_string().to_string()).block(original_block);
            }
        }
        // get the current state of the edit_string for display while editing
        let key_text =
            Paragraph::new(Span::styled(app.edit_string.clone(), active_style)).block(key_block);

        f.render_widget(original_text, sub_layout[0]);
        f.render_widget(key_text, sub_layout[1]);
        f.render_widget(alert_text, layout[1]);
    }

    // Quit pop up -----------------------------------------------------------------------------------------------------
    if app.current_screen == CurrentScreen::Exiting {
        f.render_widget(Clear, f.size()); //this clears the entire screen and anything already drawn
        let quit_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)])
            .split(area);

        let quit_block = Block::default().title("Quitting").borders(Borders::ALL);
        let quit_text = Paragraph::new(Span::styled(
            "Are you sure you wish to quit? y / n".to_string(),
            quit_style,
        ))
        .block(quit_block);
        f.render_widget(quit_text, quit_layout[0]);
    }

    // Bottom nav ------------------------------------------------------------------------------------------------------
    // it displays information about the current screen and controls for the user
    let current_navigation_text = vec![match app.current_screen {
        CurrentScreen::Main => Span::styled("Main Screen", Style::default().fg(Color::Green)),
        CurrentScreen::Editing => Span::styled("Editing Mode", Style::default().fg(Color::Yellow)),
        CurrentScreen::SoundSelection => {
            Span::styled("Sound Selection Mode", Style::default().fg(Color::Yellow))
        }
        CurrentScreen::Exiting => {
            Span::styled("Really Quit?", Style::default().fg(Color::LightRed))
        }
        CurrentScreen::Error => Span::styled("ERROR", Style::default().fg(Color::Red)),
    }
    .to_owned()];

    let mode_footer = Paragraph::new(Line::from(current_navigation_text))
        .block(Block::default().borders(Borders::ALL));

    // This displays the current keys the user can use
    // TODO: sometimes this text is scrolled off the screen on smaller terminals, figure out how to scroll it
    let current_keys_hint = {
        match app.current_screen {
            CurrentScreen::Main => Span::styled(
                "Use (arrow keys) to navigate, (enter) to select an option, or (q) to quit",
                Style::default().fg(Color::Green),
            ),
            CurrentScreen::Editing => {
                if app.currently_editing.is_some() {
                    Span::styled("Please enter a new value. Press (enter) to save, (esc) to discard changes or (q) to quit", Style::default().fg(Color::Yellow))
                } else {
                    Span::styled("Use (arrow keys) to navigate, (enter) to select, (esc) to go to main menu, or (q) to quit", Style::default().fg(Color::Yellow))
                }
            }
            CurrentScreen::SoundSelection => {
                Span::styled("Use (arrow keys) to navigate, (enter) to select, (esc) to go back to edit menu, or (q) to quit", Style::default().fg(Color::Yellow))
            },
            CurrentScreen::Exiting => Span::styled(
                "(q) to quit / (n) to return to main menu",
                Style::default().fg(Color::Red),
            ),
            CurrentScreen::Error => Span::styled(
                "Something went wrong! Please press 'q' to quit",
                Style::default().fg(Color::Red),
            ),
        }
    };

    let key_notes_footer =
        Paragraph::new(Line::from(current_keys_hint)).block(Block::default().borders(Borders::ALL));

    // here is where we create the actual footer chunks for rendering, we pass the last chunks[] element (footer)
    // to split and render those. The screen name gets 25% of the length and the hints get 75%
    let footer_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(chunks[2]);

    // Render the footer
    f.render_widget(mode_footer, footer_chunks[0]);
    f.render_widget(key_notes_footer, footer_chunks[1]);

    // Error Pop Up ----------------------------------------------------------------------------------------------------
    // hopefully no one will be seeing this :) this error pop's up if app.settings.error gets set to true by the metronome
    if app.current_screen == CurrentScreen::Error {
        f.render_widget(Clear, f.size()); //this clears the entire screen and anything already drawn
        let error_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)])
            .split(area);

        let error_block = Block::default()
            .title("Unexpected ERROR!")
            .borders(Borders::ALL);
        let error_text = Paragraph::new(Span::styled(
            "Something went wrong! Please press 'q' to quit".to_string(),
            error_style,
        ))
        .block(error_block);
        f.render_widget(error_text, error_layout[0]);
    }
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
// note: This is taken wholesale from the ratatui popup example: https://github.com/ratatui-org/ratatui/blob/main/examples/popup.rs
// it is used to create a rectangle in the center of the screen for pop ups
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    // Cut the given rectangle into three vertical pieces
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    // Then cut the middle vertical piece into three width-wise pieces
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1] // Return the middle chunk
}
