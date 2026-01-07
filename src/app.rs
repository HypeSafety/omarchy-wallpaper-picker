use crate::encoder::ImageEncoder;
use crate::wallpaper::{self, Wallpaper};
use color_eyre::Result;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::PathBuf;

pub enum Mode {
    Grid,
    Preview,
    Help,
    Search,
    Command,
}

pub struct App {
    pub wallpapers: Vec<Wallpaper>,
    pub filtered_indices: Vec<usize>,
    pub selected: usize,
    pub columns: usize,
    pub mode: Mode,
    pub should_quit: bool,
    pub current_wallpaper: Option<PathBuf>,
    pub picker: Picker,
    pub encoder: ImageEncoder,
    pub preview_state: Option<StatefulProtocol>,
    pub search_query: String,
    pub command_query: String,
    pub completions: Vec<String>,
    pub completion_index: usize,
    pub completion_dir: Option<PathBuf>,
    pub current_view_dir: Option<PathBuf>,
}

impl App {
    pub fn new() -> Result<Self> {
        let wallpapers = wallpaper::discover_wallpapers(None)?;
        let current_wallpaper = wallpaper::get_current_wallpaper();
        let picker = Picker::from_query_stdio()?;
        let encoder = ImageEncoder::new(picker.clone());

        // All indices visible initially
        let filtered_indices: Vec<usize> = (0..wallpapers.len()).collect();

        // Find the index of the current wallpaper
        let selected = current_wallpaper
            .as_ref()
            .and_then(|current| {
                wallpapers
                    .iter()
                    .position(|w| w.path == *current)
            })
            .unwrap_or(0);

        Ok(Self {
            wallpapers,
            filtered_indices,
            selected,
            columns: 4,
            mode: Mode::Grid,
            should_quit: false,
            current_wallpaper,
            picker,
            encoder,
            preview_state: None,
            search_query: String::new(),
            command_query: String::new(),
            completions: Vec::new(),
            completion_index: 0,
            completion_dir: None,
            current_view_dir: None,
        })
    }

    pub fn preload_thumbnails<F>(&mut self, mut progress: F)
    where
        F: FnMut(usize, usize, &str),
    {
        let total = self.wallpapers.len();
        for i in 0..total {
            let name = self.wallpapers[i].name.clone();
            progress(i, total, &name);
            self.wallpapers[i].load_thumbnail();
        }
    }

    pub fn update_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered_indices = (0..self.wallpapers.len()).collect();
        } else {
            self.filtered_indices = self
                .wallpapers
                .iter()
                .enumerate()
                .filter(|(_, w)| w.name.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }
        // Reset selection if out of bounds
        if self.selected >= self.filtered_indices.len() {
            self.selected = 0;
        }
    }

    pub fn start_search(&mut self) {
        self.mode = Mode::Search;
    }

    pub fn search_input(&mut self, c: char) {
        self.search_query.push(c);
        self.update_filter();
    }

    pub fn search_backspace(&mut self) {
        self.search_query.pop();
        self.update_filter();
    }

    pub fn confirm_search(&mut self) {
        self.mode = Mode::Grid;
    }

    pub fn cancel_search(&mut self) {
        self.search_query.clear();
        self.update_filter();
        self.mode = Mode::Grid;
    }

    pub fn start_command(&mut self) {
        self.mode = Mode::Command;
        self.command_query.clear();
        self.completions.clear();
    }

    pub fn command_input(&mut self, c: char) {
        self.command_query.push(c);
        self.completions.clear();
    }

    pub fn command_backspace(&mut self) {
        self.command_query.pop();
        self.completions.clear();
    }

    pub fn command_autocomplete(&mut self) {
        if !self.command_query.starts_with("cd ") {
            return;
        }

        let path_part = &self.command_query[3..];
        
        // Split into directory and partial name
        let (dir_path_str, prefix) = if let Some(last_slash) = path_part.rfind('/') {
            // If the slash is at the very end, we are looking INSIDE this directory
            if last_slash == path_part.len() - 1 {
                (path_part, "")
            } else {
                (&path_part[..=last_slash], &path_part[last_slash + 1..])
            }
        } else {
            ("", path_part)
        };

        let mut resolved_dir_str = dir_path_str.to_string();
        if resolved_dir_str.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                resolved_dir_str = resolved_dir_str.replacen('~', &home.to_string_lossy(), 1);
            }
        }
        
        let search_dir = if resolved_dir_str.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(resolved_dir_str)
        };

        // If we are cycling (completions not empty and we are within the same search dir),
        // just move to the next completion.
        if !self.completions.is_empty() {
            if let Some(ref last_dir) = self.completion_dir {
                if *last_dir == search_dir {
                    self.completion_index = (self.completion_index + 1) % self.completions.len();
                    self.command_query = self.completions[self.completion_index].clone();
                    return;
                }
            }
        }

        // Otherwise, fetch new completions
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            let mut matches = Vec::new();
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.starts_with(prefix) {
                                matches.push(format!("cd {}{}/", dir_path_str, name));
                            }
                        }
                    }
                }
            }
            matches.sort();

            if !matches.is_empty() {
                self.completion_dir = Some(search_dir);
                if matches.len() == 1 {
                    // Unique match: complete it
                    self.command_query = matches[0].clone();
                    
                    // Immediately look inside this new directory
                    let next_path = &self.command_query[3..];
                    let mut resolved_next = next_path.to_string();
                    if resolved_next.starts_with('~') {
                        if let Some(home) = dirs::home_dir() {
                            resolved_next = resolved_next.replacen('~', &home.to_string_lossy(), 1);
                        }
                    }
                    let next_dir = PathBuf::from(resolved_next);
                    
                    let mut sub_matches = Vec::new();
                    if let Ok(sub_entries) = std::fs::read_dir(&next_dir) {
                        for sub_entry in sub_entries.flatten() {
                            if let Ok(sub_ft) = sub_entry.file_type() {
                                if sub_ft.is_dir() {
                                    if let Some(sub_name) = sub_entry.file_name().to_str() {
                                        sub_matches.push(format!("cd {}{}/", next_path, sub_name));
                                    }
                                }
                            }
                        }
                    }
                    sub_matches.sort();
                    self.completions = sub_matches;
                    self.completion_index = 0;
                    // Note: We don't update command_query again so it stays at the completed dir
                    // but shows the sub-options in the list.
                } else {
                    // Multiple matches: cycle through them
                    self.completions = matches;
                    self.completion_index = 0;
                    self.command_query = self.completions[0].clone();
                }
            }
        }
    }

    pub fn move_completion_down(&mut self) {
        if !self.completions.is_empty() {
            self.completion_index = (self.completion_index + 1) % self.completions.len();
            self.command_query = self.completions[self.completion_index].clone();
        }
    }

    pub fn move_completion_up(&mut self) {
        if !self.completions.is_empty() {
            if self.completion_index == 0 {
                self.completion_index = self.completions.len() - 1;
            } else {
                self.completion_index -= 1;
            }
            self.command_query = self.completions[self.completion_index].clone();
        }
    }

    pub fn confirm_command(&mut self) -> Result<()> {
        let cmd = self.command_query.trim();
        if cmd.starts_with("cd ") {
            let mut path_str = cmd[3..].trim().to_string();
            if path_str.starts_with('~') {
                if let Some(home) = dirs::home_dir() {
                    path_str = path_str.replacen('~', &home.to_string_lossy(), 1);
                }
            }
            let path = PathBuf::from(path_str);
            self.current_view_dir = Some(path);
            self.reload_wallpapers()?;
        } else if cmd == "cd" {
            self.current_view_dir = None;
            self.reload_wallpapers()?;
        }
        self.mode = Mode::Grid;
        self.command_query.clear();
        Ok(())
    }

    pub fn reload_wallpapers(&mut self) -> Result<()> {
        self.wallpapers = wallpaper::discover_wallpapers(self.current_view_dir.clone())?;
        self.encoder.clear_cache();
        self.preview_state = None;
        self.update_filter();
        self.selected = 0;
        Ok(())
    }

    pub fn cancel_command(&mut self) {
        self.command_query.clear();
        self.mode = Mode::Grid;
    }

    pub fn reset_view_dir(&mut self) -> Result<()> {
        self.current_view_dir = None;
        self.reload_wallpapers()
    }

    pub fn move_up(&mut self) {
        if self.selected >= self.columns {
            self.selected -= self.columns;
        }
    }

    pub fn move_down(&mut self) {
        let new_pos = self.selected + self.columns;
        if new_pos < self.filtered_indices.len() {
            self.selected = new_pos;
        }
    }

    pub fn move_left(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
        }
    }

    pub fn toggle_preview(&mut self) {
        match self.mode {
            Mode::Grid => {
                self.preview_state = None; // Reset preview state for new image
                self.mode = Mode::Preview;
            }
            Mode::Preview => self.mode = Mode::Grid,
            Mode::Help | Mode::Search | Mode::Command => {}
        }
    }

    pub fn toggle_help(&mut self) {
        match self.mode {
            Mode::Help => self.mode = Mode::Grid,
            _ => self.mode = Mode::Help,
        }
    }

    pub fn apply_wallpaper(&mut self) -> Result<()> {
        if let Some(&idx) = self.filtered_indices.get(self.selected) {
            if let Some(wallpaper) = self.wallpapers.get(idx) {
                // Install to omarchy backgrounds dir and get the path
                let installed_path = wallpaper::install_wallpaper(wallpaper)?;

                // Set as current wallpaper (symlink)
                wallpaper::set_wallpaper(&installed_path)?;
                self.current_wallpaper = Some(installed_path);
            }
        }
        Ok(())
    }

    pub fn escape(&mut self) {
        match self.mode {
            Mode::Preview | Mode::Help => self.mode = Mode::Grid,
            Mode::Search => self.cancel_search(),
            Mode::Command => self.cancel_command(),
            Mode::Grid => self.should_quit = true,
        }
    }

    pub fn selected_wallpaper(&self) -> Option<&Wallpaper> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.wallpapers.get(idx))
    }

    pub fn is_current(&self, index: usize) -> bool {
        self.current_wallpaper
            .as_ref()
            .map(|current| {
                self.wallpapers
                    .get(index)
                    .map(|w| w.path == *current)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }
}
