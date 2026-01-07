use crate::app::{App, Mode};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};
use ratatui_image::{StatefulImage, Resize};

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Main layout: content + status/search bar
    let bottom_height = if matches!(app.mode, Mode::Search) { 3 } else { 1 };
    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(bottom_height)]).split(area);

    render_grid(frame, app, chunks[0]);

    match app.mode {
        Mode::Search => render_search_bar(frame, app, chunks[1]),
        _ => render_status_bar(frame, app, chunks[1]),
    }

    // Render modal overlays
    match app.mode {
        Mode::Preview => render_preview_modal(frame, app, area),
        Mode::Help => render_help_modal(frame, area),
        Mode::Command => render_command_modal(frame, app, area),
        Mode::Grid | Mode::Search => {}
    }
}

fn render_grid(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = if app.search_query.is_empty() {
        " Wallpapers ".to_string()
    } else {
        format!(" Wallpapers ({} matches) ", app.filtered_indices.len())
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.filtered_indices.is_empty() {
        let msg = if app.search_query.is_empty() {
            "No wallpapers found"
        } else {
            "No matches found"
        };
        let msg = Paragraph::new(msg)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, inner);
        return;
    }

    // Reserve 1 column for scrollbar
    let grid_width = inner.width.saturating_sub(1);

    // Calculate columns based on window width
    // Target a minimum cell width of 30 chars for readable thumbnails
    const MIN_CELL_WIDTH: u16 = 30;
    const MAX_COLUMNS: usize = 8;
    const MIN_COLUMNS: usize = 1;

    let columns = ((grid_width / MIN_CELL_WIDTH) as usize)
        .clamp(MIN_COLUMNS, MAX_COLUMNS);

    // Update app.columns so navigation works correctly
    app.columns = columns;

    let cell_width = grid_width / columns as u16;
    // Terminal cells are ~2:1 (height:width in pixels)
    let cell_height = cell_width / 2;

    if cell_height == 0 {
        return;
    }

    let total_items = app.filtered_indices.len();
    let total_rows = (total_items + columns - 1) / columns;
    let selected_row = app.selected / columns;

    // Calculate visible rows (including partial)
    let visible_full_rows = inner.height / cell_height;
    let has_partial = inner.height % cell_height > 0;
    let visible_rows = visible_full_rows as usize + if has_partial { 1 } else { 0 };

    // Scroll offset - keep selected row visible
    let scroll_offset = if selected_row < visible_full_rows as usize / 2 {
        0
    } else if selected_row >= total_rows.saturating_sub(visible_full_rows as usize / 2) {
        total_rows.saturating_sub(visible_full_rows as usize)
    } else {
        selected_row.saturating_sub(visible_full_rows as usize / 2)
    };

    // Render grid cells
    for row in 0..visible_rows {
        let actual_row = scroll_offset + row;
        if actual_row >= total_rows {
            break;
        }

        for col in 0..columns {
            let filtered_pos = actual_row * columns + col;
            if filtered_pos >= total_items {
                break;
            }

            let x = inner.x + (col as u16 * cell_width);
            let y = inner.y + (row as u16 * cell_height);

            // Calculate available height for this cell (may be partial)
            let available_height = (inner.y + inner.height).saturating_sub(y);
            let this_cell_height = cell_height.min(available_height);

            if this_cell_height < 3 {
                continue; // Too small to render
            }

            let cell_area = Rect::new(x, y, cell_width.saturating_sub(1), this_cell_height.saturating_sub(1));
            render_wallpaper_cell(frame, app, filtered_pos, cell_area);
        }
    }

    // Render scrollbar
    if total_rows > visible_full_rows as usize {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        let mut scrollbar_state = ScrollbarState::new(total_rows)
            .position(scroll_offset);

        let scrollbar_area = Rect::new(
            inner.x + inner.width - 1,
            inner.y,
            1,
            inner.height,
        );

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn render_wallpaper_cell(frame: &mut Frame, app: &mut App, filtered_pos: usize, area: Rect) {
    if area.width < 3 || area.height < 3 {
        return;
    }

    let original_index = match app.filtered_indices.get(filtered_pos) {
        Some(&idx) => idx,
        None => return,
    };

    // Clone what we need before mutable borrows
    let name = app.wallpapers[original_index].name.clone();
    let is_selected = filtered_pos == app.selected;
    let is_current = app.is_current(original_index);

    let border_color = if is_selected {
        Color::Yellow
    } else if is_current {
        Color::Green
    } else {
        Color::DarkGray
    };

    let border_style = if is_selected {
        Style::default().fg(border_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(border_color)
    };

    let title = if is_current { " ✓ " } else { "" };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Render image
    if inner.width > 0 && inner.height > 1 {
        // Use full area minus bottom row for filename
        // Resize::Fit will scale the thumbnail up and center it
        let image_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));

        // Create protocol if not cached, or use cached
        if !app.image_states.contains_key(&original_index) {
            // Load thumbnail lazily if missing
            if app.wallpapers[original_index].thumbnail.is_none() {
                app.wallpapers[original_index].load_thumbnail();
            }
            if let Some(ref thumb) = app.wallpapers[original_index].thumbnail {
                let protocol = app.picker.new_resize_protocol(thumb.clone());
                app.image_states.insert(original_index, protocol);
            }
        }

        if let Some(state) = app.image_states.get_mut(&original_index) {
            let image = StatefulImage::new(None).resize(Resize::Fit(None));
            frame.render_stateful_widget(image, image_area, state);
        }

        // Render filename below image
        let name_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
        let display_name = truncate_name(&name, inner.width as usize);
        let name_style = if is_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let name_widget = Paragraph::new(display_name)
            .style(name_style)
            .alignment(Alignment::Center);
        frame.render_widget(name_widget, name_area);
    }
}

fn render_preview_modal(frame: &mut Frame, app: &mut App, area: Rect) {
    let modal_area = centered_rect(80, 80, area);

    frame.render_widget(Clear, modal_area);

    let wallpaper = match app.selected_wallpaper() {
        Some(w) => w,
        None => return,
    };

    let block = Block::default()
        .title(format!(" {} ", wallpaper.name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    // Load preview image if needed
    if app.preview_state.is_none() {
        if let Ok(dyn_img) = image::open(&wallpaper.path) {
            let protocol = app.picker.new_resize_protocol(dyn_img);
            app.preview_state = Some(protocol);
        }
    }

    if let Some(state) = app.preview_state.as_mut() {
        let image = StatefulImage::new(None).resize(Resize::Fit(None));
        frame.render_stateful_widget(image, inner, state);
    }
}

fn render_help_modal(frame: &mut Frame, area: Rect) {
    let modal_area = centered_rect(50, 75, area);

    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    let help_text = vec![
        Line::from(vec![
            Span::styled("Navigation", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ↑/k  ", Style::default().fg(Color::Cyan)),
            Span::raw("Move up"),
        ]),
        Line::from(vec![
            Span::styled("  ↓/j  ", Style::default().fg(Color::Cyan)),
            Span::raw("Move down"),
        ]),
        Line::from(vec![
            Span::styled("  ←/h  ", Style::default().fg(Color::Cyan)),
            Span::raw("Move left"),
        ]),
        Line::from(vec![
            Span::styled("  →/l  ", Style::default().fg(Color::Cyan)),
            Span::raw("Move right"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Actions", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Enter  ", Style::default().fg(Color::Cyan)),
            Span::raw("Apply wallpaper"),
        ]),
        Line::from(vec![
            Span::styled("  Space  ", Style::default().fg(Color::Cyan)),
            Span::raw("Preview wallpaper"),
        ]),
        Line::from(vec![
            Span::styled("  /      ", Style::default().fg(Color::Cyan)),
            Span::raw("Search/filter"),
        ]),
        Line::from(vec![
            Span::styled("  :      ", Style::default().fg(Color::Cyan)),
            Span::raw("Open command mode"),
        ]),
        Line::from(vec![
            Span::styled("  H      ", Style::default().fg(Color::Cyan)),
            Span::raw("Reset view dir"),
        ]),
        Line::from(vec![
            Span::styled("  ?      ", Style::default().fg(Color::Cyan)),
            Span::raw("Toggle help"),
        ]),
        Line::from(vec![
            Span::styled("  Esc    ", Style::default().fg(Color::Cyan)),
            Span::raw("Close modal / Exit"),
        ]),
        Line::from(vec![
            Span::styled("  q      ", Style::default().fg(Color::Cyan)),
            Span::raw("Quit"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Commands", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  :cd <path>  ", Style::default().fg(Color::Cyan)),
            Span::raw("Browse wallpapers in directory"),
        ]),
        Line::from(vec![
            Span::styled("  :cd         ", Style::default().fg(Color::Cyan)),
            Span::raw("Reset to default directory"),
        ]),
    ];

    let help = Paragraph::new(help_text).wrap(Wrap { trim: false });
    frame.render_widget(help, inner);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let filter_info = if app.search_query.is_empty() {
        format!("{} wallpapers", app.wallpapers.len())
    } else {
        format!("{}/{} (filter: {})", app.filtered_indices.len(), app.wallpapers.len(), app.search_query)
    };

    let dir_info = if let Some(ref dir) = app.current_view_dir {
        format!(" | dir: {} ", dir.display())
    } else {
        " | dir: default ".to_string()
    };

    let status = format!(
        " {} | Selected: {} | / search | : cmd | ? help | q quit{}",
        filter_info,
        app.selected + 1,
        dir_info
    );

    let status_bar = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(status_bar, area);
}

fn render_search_bar(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let search_text = format!("/{}_", app.search_query);
    let search = Paragraph::new(search_text)
        .style(Style::default().fg(Color::White));

    frame.render_widget(search, inner);
}

fn render_command_modal(frame: &mut Frame, app: &App, area: Rect) {
    let modal_width = 60;
    let modal_height = 3 + if app.completions.is_empty() { 0 } else { (app.completions.len().min(10) as u16) + 2 };
    
    let modal_area = Rect::new(
        (area.width.saturating_sub(modal_width)) / 2,
        area.height / 3, // Position it a bit higher than center
        modal_width.min(area.width),
        modal_height.min(area.height),
    );

    frame.render_widget(Clear, modal_area);

    let chunks = if app.completions.is_empty() {
        vec![modal_area]
    } else {
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
        ]).split(modal_area).to_vec()
    };

    // Command Input
    let block = Block::default()
        .title(" Command ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(chunks[0]);
    frame.render_widget(block, chunks[0]);

    let command_text = format!(":{}_", app.command_query);
    let command = Paragraph::new(command_text)
        .style(Style::default().fg(Color::White));
    frame.render_widget(command, inner);

    // Completions
    if !app.completions.is_empty() {
        let comp_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let comp_inner = comp_block.inner(chunks[1]);
        frame.render_widget(comp_block, chunks[1]);

        let visible_count = comp_inner.height as usize;
        let total_count = app.completions.len();
        
        let scroll_offset = if total_count <= visible_count {
            0
        } else if app.completion_index < visible_count / 2 {
            0
        } else if app.completion_index >= total_count.saturating_sub(visible_count / 2) {
            total_count.saturating_sub(visible_count)
        } else {
            app.completion_index.saturating_sub(visible_count / 2)
        };

        let list_items: Vec<Line> = app.completions.iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_count)
            .map(|(i, c)| {
                if i == app.completion_index {
                    Line::from(vec![
                        Span::styled(" > ", Style::default().fg(Color::Yellow)),
                        Span::styled(c, Style::default().bg(Color::Cyan).fg(Color::Black)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw("   "),
                        Span::raw(c),
                    ])
                }
            }).collect();

        let list = Paragraph::new(list_items);
        frame.render_widget(list, comp_inner);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

fn truncate_name(name: &str, max_width: usize) -> String {
    if name.len() <= max_width {
        name.to_string()
    } else if max_width > 3 {
        format!("{}...", &name[..max_width - 3])
    } else {
        name[..max_width].to_string()
    }
}
