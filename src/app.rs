/// App.rs holds the current application state of Ready Metronome. It keeps track of the current screen, quitting,
/// and various settings on the metronome like the bpm, volume and whether or not it is playing. It is additionally
/// in charge of starting the metronome thread and keeping a reference to it's handle
// App.rs is loosely based on the ratatui JSON editor tutorial found here: https://ratatui.rs/tutorials/json-editor/app/
use crate::{
    menu::Menu,
    metronome::{InitMetronomeSettings, Metronome, MetronomeSettings},
};
use atomic_float::AtomicF64;
use color_eyre::{eyre::eyre, Report, Result};
use crossterm::event::{KeyCode, KeyEvent};
use std::sync::Arc;
use std::thread;
use std::{
    fs,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

// These two enums are used extensively in events.rs and ui.rs to render the correct state and
// select the right value when editing
#[derive(PartialEq)]
pub enum CurrentScreen {
    Main,
    Editing,
    Exiting,
    SoundSelection,
    Error,
}

#[derive(Clone, Copy)]
pub enum CurrentlyEditing {
    Bpm,
    Volume,
    TimeSignature,
}

pub struct App {
    pub settings: MetronomeSettings,
    pub current_screen: CurrentScreen,
    pub currently_editing: Option<CurrentlyEditing>,
    pub metronome_handle: Option<thread::JoinHandle<()>>,
    pub edit_string: String,
    pub alert_string: String,
    pub main_menu: Menu,
    pub edit_menu: Menu,
    pub sound_selection_menu: Menu,
    pub should_quit: bool,
    pub first_edit: bool, // this is used to overwrite the original metronome setting text upon opening the edit window
    pub sound_list: Vec<String>,
    pub tick_rate: u64,
    pub volume_min: f64,
    pub volume_max: f64,
    pub bpm_min: u64,
    pub bpm_max: u64,
}

impl App {
    pub fn new(init_settings: InitMetronomeSettings, set_tick_rate: u64) -> App {
        App {
            settings: MetronomeSettings {
                bpm: Arc::new(AtomicU64::new(init_settings.bpm)),
                ns_delay: Arc::new(AtomicU64::new(500_000_000)),
                ts_note: Arc::new(AtomicU64::new(init_settings.ts_note)),
                ts_value: Arc::new(AtomicU64::new(init_settings.ts_value)),
                ts_triplets: Arc::new(AtomicBool::new(false)),
                sub_eights: Arc::new(AtomicBool::new(false)),
                sub_sixteens: Arc::new(AtomicBool::new(false)),
                current_beat_count: Arc::new(AtomicU64::new(0)),
                beats_per_bar: Arc::new(AtomicU64::new(4)),
                bar_count: Arc::new(AtomicU64::new(1)),
                is_running: Arc::new(AtomicBool::new(init_settings.is_running)),
                volume: Arc::new(AtomicF64::new(init_settings.volume)),
                sound_list: Vec::new(),
                selected_sound: Arc::new(AtomicUsize::new(0)),
                tick_count: Arc::new(AtomicU64::new(0)),
                debug: Arc::new(AtomicBool::new(init_settings.debug)),
                error: Arc::new(AtomicBool::new(false)),
            },
            current_screen: CurrentScreen::Main,
            currently_editing: None,
            metronome_handle: None,
            edit_string: String::new(),
            alert_string: String::new(),
            main_menu: Menu::new(vec![
                "Start / Stop Metronome".to_string(),
                "Edit Metronome Settings".to_string(),
                "Quit".to_string(),
            ]),
            edit_menu: Menu::new(vec![]),
            sound_selection_menu: Menu::new(vec![]),
            should_quit: false,
            first_edit: true,
            sound_list: Vec::new(),
            tick_rate: set_tick_rate,
            volume_min: 1.0,
            volume_max: 300.0,
            bpm_min: 20,
            bpm_max: 500,
        }
    }

    pub fn init(&mut self) {
        match self.populate_sounds() {
            Ok(()) => {
                self.spawn_metronome_thread();
                self.main_menu.select(0);
            }
            Err(error) => {
                println!("Problem populating sounds: {}", error);
                self.settings.error.swap(true, Ordering::Relaxed);
            }
        };
        let ns_delay = self.get_ns_for_note_value();
        self.settings.ns_delay.swap(ns_delay, Ordering::Relaxed);
        self.update_beats_per_bar();
    }

    fn populate_sounds(&mut self) -> Result<(), Report> {
        // loop through sounds found in /assets and add them to the sound_list vec
        // TODO: In the future, nested sound directories could be nice to organize by type
        if let Ok(entries) = fs::read_dir("./assets/") {
            for entry in entries {
                let string: String = entry?.file_name().into_string().unwrap();
                self.sound_list.push(string);
            }
        }

        // clone these over to the metronome settings vec prior to spawning metronome thread
        self.settings.sound_list = self.sound_list.clone();

        Ok(())
    }

    // Spawns a metronome on its own thread
    fn spawn_metronome_thread(&mut self) {
        let mut metronome = Metronome::new(&self.settings);
        let tick_rate_copy = self.tick_rate;
        self.metronome_handle = Some(thread::spawn(move || {
            metronome.start(tick_rate_copy);
        }));
        self.check_error_status();
    }

    // Metronome settings change functions
    pub fn change_bpm(&mut self, new_bpm: u64) {
        if !(self.u64_in_range(new_bpm, self.bpm_min, self.bpm_max)) {
            return;
        }
        self.settings.bpm.swap(new_bpm, Ordering::Relaxed);
        let new_ns = self.get_ns_for_note_value();
        self.settings.ns_delay.swap(new_ns, Ordering::Relaxed);
    }

    pub fn u64_in_range(&mut self, value: u64, low: u64, high: u64) -> bool
    {
        if (low..=high).contains(&value) {
            return true;
        }
        false
    }

    pub fn f64_in_range(&mut self, value: f64, low: f64, high: f64) -> bool
    {
        if (low..=high).contains(&value) {
            return true;
        }
        false
    }

    // functions to change these values from the editor
    pub fn change_bpm_editor(&mut self) -> bool {
        if self.edit_string.is_empty() {
            false
        } else {
            let new_bpm: u64 = match self.edit_string.parse() {
                Ok(new_value) => new_value,
                Err(_) => return false,
            };
            if self.u64_in_range(new_bpm, self.bpm_min, self.bpm_max) {
                self.settings.bpm.swap(new_bpm, Ordering::Relaxed);
                let new_ns_delay = self.get_ns_for_note_value();
                self.settings.ns_delay.swap(new_ns_delay, Ordering::Relaxed);
                self.clear_strings();
                self.currently_editing = None;
                true
            } else {
                self.edit_string.clear();
                false
            }
        }
    }

    pub fn change_volume_editor(&mut self) -> bool {
        if self.edit_string.is_empty() {
            false
        } else {
            let new_volume: f64 = match self.edit_string.parse() {
                Ok(new_value) => new_value,
                Err(_) => return false,
            };
            if self.f64_in_range(new_volume, self.volume_min, self.volume_max) {
                self.settings.volume.swap(new_volume, Ordering::Relaxed);
                self.clear_strings();
                self.currently_editing = None;
                true
            } else {
                self.edit_string.clear();
                false
            }
        }
    }

    pub fn change_signature(&mut self) -> bool {
        if self.edit_string.is_empty() {
            false
        } else {
            // TODO: This is pretty restrictive
            let v: Vec<&str> = self.edit_string.split('/').collect();
            let new_ts_beats = match v[0].parse() {
                Ok(new_value) => new_value,
                Err(_) => return false,
            };
            let new_ts_value = match v[1].parse() {
                Ok(new_value) => new_value,
                Err(_) => return false,
            };
            let new_ns = self.get_ns_for_note_value();

            self.settings.ts_note.swap(new_ts_beats, Ordering::Relaxed);
            self.settings.ts_value.swap(new_ts_value, Ordering::Relaxed);
            self.settings.ns_delay.swap(new_ns, Ordering::Relaxed);
            self.update_beats_per_bar();

            self.clear_strings();
            self.currently_editing = None;
            true
        }
    }

    pub fn toggle_metronome(&mut self) {
        let currently_playing = self.settings.is_running.load(Ordering::Relaxed);
        self.settings
            .is_running
            .swap(!currently_playing, Ordering::Relaxed);
        // This will trigger if the metronome fails to load a file
        self.check_error_status();
    }

    // Convert a bpm value to the nanosecond delay (1/4 notes)
    fn get_ns_from_bpm(&mut self) -> u64 {
        (60_000_000_000.0_f64 / self.settings.bpm.load(Ordering::Relaxed) as f64).round() as u64
    }

    // Take the current nanosecond delay and divide it based on the value note in the time signature
    fn get_ns_for_note_value(&mut self) -> u64 {
        let value = self.settings.ts_value.load(Ordering::Relaxed);
        let mut current_ns_delay = self.get_ns_from_bpm(); // length of a quarter note

        // Handle triplet meters like 12/8
        if value == 8 {
            current_ns_delay = (current_ns_delay as f64 / 3_f64).round() as u64;
        } else if value != 4 {
            current_ns_delay = match value {
                64 => (current_ns_delay as f64 / 16_f64).round() as u64,
                32 => (current_ns_delay as f64 / 8_f64).round() as u64,
                16 => (current_ns_delay as f64 / 4_f64).round() as u64,
                8 => (current_ns_delay as f64 / 2_f64).round() as u64,
                _ => current_ns_delay,
            }
        }
        // Calculate 8ths or 16ths subdivision in 4/4
        if value == 4 {
            if self.settings.sub_eights.load(Ordering::Relaxed) {
                current_ns_delay = (current_ns_delay as f64 / 2_f64).round() as u64;
            } else if self.settings.sub_sixteens.load(Ordering::Relaxed) {
                current_ns_delay = (current_ns_delay as f64 / 4_f64).round() as u64;
            }
            // This was helpful in thinking about triplet calculation:
            // https://math.stackexchange.com/questions/2646908/calculating-delay-time-in-milliseconds
            if self.settings.ts_triplets.load(Ordering::Relaxed) {
                current_ns_delay = (current_ns_delay as f64 / 3_f64 * 2_f64).round() as u64;
            }
        }

        current_ns_delay
    }

    // Calculate and return the number of metronome beats per bar (based on time signature and subdivision)
    fn update_beats_per_bar(&mut self) {
        let mut num_ticks = self.settings.ts_note.load(Ordering::Relaxed);
        if self.settings.ts_triplets.load(Ordering::Relaxed) {
            num_ticks = (num_ticks as f64 * 1.5_f64).round() as u64;
        }
        if self.settings.sub_eights.load(Ordering::Relaxed) {
            num_ticks *= 2;
        } else if self.settings.sub_sixteens.load(Ordering::Relaxed) {
            num_ticks *= 4;
        }
        self.settings
            .beats_per_bar
            .swap(num_ticks, Ordering::Relaxed);
    }

    pub fn clear_strings(&mut self) {
        self.alert_string.clear();
        self.edit_string.clear();
    }

    pub fn check_error_status(&mut self) {
        if self.settings.error.load(Ordering::Relaxed) {
            self.current_screen = CurrentScreen::Error;
        }
    }

    pub fn refresh_edit_menu(&mut self) {
        let edit_menu_selection = self.edit_menu.state.selected();
        let is_playing = if self.get_is_running() { "yes" } else { "no" };
        let mut edit_menu_vec = vec![
            "playing: ".to_owned() + is_playing,
            "bpm: ".to_owned() + &self.get_bpm().to_string(),
            "volume: ".to_owned() + &self.get_volume().to_string(),
            "select sound: ".to_owned() + &self.get_selected_sound_string(),
            "Time signature: ".to_owned() + &self.get_time_sig_string(),
            "Bar count: ".to_owned() + &self.get_bar_count_string(),
            "Back to main menu".to_owned(),
        ];
        // Add debug displays
        if self.settings.debug.load(Ordering::Relaxed) {
            edit_menu_vec.push("\n// DEBUG // ".to_owned());
            edit_menu_vec.push(
                "TICK COUNT: ".to_owned()
                    + &self.settings.tick_count.load(Ordering::Relaxed).to_string(),
            );
            edit_menu_vec.push(
                "Current NS Delay: ".to_owned()
                    + &self.settings.ns_delay.load(Ordering::Relaxed).to_string(),
            );
        }
        self.edit_menu.set_items(edit_menu_vec);

        // clippy hates this no matter what I do...
        if let Some(..) = edit_menu_selection {
            self.edit_menu.select(edit_menu_selection.unwrap());
        }
    }

    pub fn refresh_sound_selection_menu(&mut self) {
        // list sounds
        self.sound_selection_menu.set_items(self.sound_list.clone());
        // select the current sound
        self.sound_selection_menu
            .select(self.settings.selected_sound.load(Ordering::Relaxed));
    }

    // TODO: Separate ui nav code from app -----------------------------------------------------------------------------
    pub fn update(&mut self, key: KeyEvent) -> Result<String, Report> {
        let mut ask_for_quit = false; // used to prevent pressing q to quit entire program with no warning

        // If in error mode, return error
        if self.settings.error.load(Ordering::Relaxed) {
            return Err(eyre!("App.update() Something went wrong!"));
        }
        // global keyboard shortcuts and menu navigation controls
        match key.code {
            // navigate menu items
            KeyCode::Up
            | KeyCode::Left
            | KeyCode::BackTab
            | KeyCode::Down
            | KeyCode::Right
            | KeyCode::Tab
            | KeyCode::Esc => {
                self.menu_navigate(key);
            }
            KeyCode::Char('+') => {
                let old_bpm = self.get_bpm();
                self.change_bpm(old_bpm + 10);
            }
            KeyCode::Char('-') => {
                let old_bpm = self.get_bpm();
                self.change_bpm(old_bpm - 10);
            }
            // toggle metronome on/off
            KeyCode::Char('t') => {
                if self.currently_editing.is_none() {
                    self.toggle_metronome();
                }
            }
            // quit at any time
            KeyCode::Char('q') => {
                if self.current_screen != CurrentScreen::Exiting {
                    self.current_screen = CurrentScreen::Exiting;
                    self.edit_menu.deselect();
                    self.currently_editing = None;
                    self.clear_strings();
                    ask_for_quit = true;
                }
            }
            _ => {}
        }

        // Screen specific keyboard shortcuts
        // Main screen ---------------------------------------------------------------------------------------------
        match self.current_screen {
            CurrentScreen::Main => {
                if key.code == KeyCode::Enter {
                    let current_selection = self.main_menu.state.selected().unwrap();
                    // TODO: This is messy and bad, magic numbers are not scalable
                    match current_selection {
                        0 => {
                            // start / stop metronome
                            self.toggle_metronome();
                        }
                        1 => {
                            // enter edit menu
                            self.switch_screen(CurrentScreen::Editing);
                        }
                        2 => {
                            // enter quit menu
                            self.current_screen = CurrentScreen::Exiting;
                        }
                        _ => {}
                    }
                }
            }
            // Edit screen -----------------------------------------------------------------------------------------
            CurrentScreen::Editing => match key.code {
                // When editing a value, add / remove characters from the edit_string
                KeyCode::Char(value) => {
                    if self.currently_editing.is_some() {
                        if self.first_edit {
                            self.edit_string.clear();
                            self.first_edit = false;
                        }
                        self.edit_string.push(value);
                    }
                }
                KeyCode::Backspace => {
                    if self.currently_editing.is_some() {
                        self.edit_string.pop();
                    }
                }
                // When editing a value, save the result or retry if failed
                KeyCode::Enter => {
                    if let Some(editing) = &self.currently_editing {
                        match editing {
                            CurrentlyEditing::Bpm => {
                                if self.change_bpm_editor() {
                                    self.edit_menu.select(1);
                                    self.first_edit = true;
                                } else {
                                    self.alert_string =
                                        "Please input a value between 20 and 500".to_owned();
                                }
                            }
                            CurrentlyEditing::Volume => {
                                if self.change_volume_editor() {
                                    self.edit_menu.select(2);
                                    self.first_edit = true;
                                } else {
                                    self.alert_string =
                                        "Please input a value between 1.0 and 200.0".to_owned();
                                }
                            }
                            CurrentlyEditing::TimeSignature => {
                                if self.change_signature() {
                                    self.edit_menu.select(3);
                                    self.first_edit = true;
                                } else {
                                    self.alert_string =
                                        "Something went wrong, make sure to use the format X/X"
                                            .to_owned();
                                }
                            }
                        }
                    } else {
                        // Main edit menu --------------------------------------------
                        // TODO: This is messy and bad, magic numbers are not scalable
                        let current_selection = self.edit_menu.state.selected().unwrap();
                        match current_selection {
                            0 => {
                                // start / stop metronome
                                self.toggle_metronome()
                            }
                            1 => {
                                // edit bpm
                                self.edit_string = self.get_bpm().to_string();
                                self.currently_editing = Some(CurrentlyEditing::Bpm);
                                self.edit_menu.deselect();
                            }
                            2 => {
                                // edit volume
                                self.edit_string = self.get_volume().to_string();
                                self.currently_editing = Some(CurrentlyEditing::Volume);
                                self.edit_menu.deselect();
                            }
                            3 => {
                                // sound selection menu
                                self.switch_screen(CurrentScreen::SoundSelection);
                            }
                            4 => {
                                // edit time signature
                                self.edit_string = self.get_time_sig_string();
                                self.currently_editing = Some(CurrentlyEditing::TimeSignature);
                                self.edit_menu.deselect();
                            }
                            5 => {
                                // bar count display, do nothing
                            }
                            6 => {
                                // back to main menu
                                self.switch_screen(CurrentScreen::Main);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            // Sound Selection Screen ------------------------------------------------------------------------------
            CurrentScreen::SoundSelection => {
                if key.code == KeyCode::Enter {
                    let selection = self.sound_selection_menu.state.selected().unwrap();
                    if selection <= self.sound_list.len() {
                        self.settings
                            .selected_sound
                            .swap(selection, Ordering::Relaxed);
                    }
                    self.switch_screen(CurrentScreen::Editing);
                }
            }
            // Exit screen -----------------------------------------------------------------------------------------
            CurrentScreen::Exiting => match key.code {
                KeyCode::Char('y') | KeyCode::Char('q') | KeyCode::Enter => {
                    // Quit
                    if !ask_for_quit {
                        self.should_quit = true;
                    }
                }
                KeyCode::Char('n') | KeyCode::Backspace | KeyCode::Esc | KeyCode::Tab => {
                    // Reset the menu state to a default value
                    self.current_screen = CurrentScreen::Main;
                    self.currently_editing = None;
                    self.clear_strings();
                    self.first_edit = true;
                    self.main_menu.select(0);
                }
                _ => {}
            },
            // Error screen ----------------------------------------------------------------------------------------
            CurrentScreen::Error => {
                // Press any char to quit, could not find an "any" keybind in Crossterm
                if let KeyCode::Char(_) = key.code {
                    return Err(eyre!(
                        "ReadyMetronome experienced a terminal error! Sorry about that..."
                    ));
                }
            }
        }

        Ok("App updated".to_string())
    }

    fn switch_screen(&mut self, new_screen: CurrentScreen) {
        match new_screen {
            CurrentScreen::Main => {
                self.edit_menu.deselect();
                self.sound_selection_menu.deselect();
                self.first_edit = true;
                if self.current_screen == CurrentScreen::Editing {
                    self.main_menu.select(1);
                } else {
                    self.main_menu.select(0);
                }
            }
            CurrentScreen::Editing => {
                self.main_menu.deselect();
                self.sound_selection_menu.deselect();
                self.edit_menu.select(0);
            }
            CurrentScreen::SoundSelection => {
                self.main_menu.deselect();
                self.edit_menu.deselect();
                self.refresh_sound_selection_menu();
            }
            CurrentScreen::Exiting => {
                self.main_menu.deselect();
                self.edit_menu.deselect();
                self.sound_selection_menu.deselect();
                self.currently_editing = None;
                self.clear_strings();
            }
            CurrentScreen::Error => {
                // Probably unnecessary but might as well while I'm here?
                self.main_menu.deselect();
                self.edit_menu.deselect();
                self.sound_selection_menu.deselect();
            }
        }
        self.current_screen = new_screen;
    }

    fn menu_navigate(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Left | KeyCode::BackTab => match self.current_screen {
                CurrentScreen::Main => {
                    self.main_menu.previous();
                }
                CurrentScreen::Editing => {
                    if self.currently_editing.is_none() {
                        self.edit_menu.previous();
                    }
                }
                CurrentScreen::SoundSelection => {
                    self.sound_selection_menu.previous();
                }
                CurrentScreen::Exiting => {}
                CurrentScreen::Error => {}
            },
            KeyCode::Down | KeyCode::Right | KeyCode::Tab => match self.current_screen {
                CurrentScreen::Main => {
                    self.main_menu.next();
                }
                CurrentScreen::Editing => {
                    if self.currently_editing.is_none() {
                        self.edit_menu.next();
                    }
                }
                CurrentScreen::SoundSelection => {
                    self.sound_selection_menu.next();
                }
                CurrentScreen::Exiting => {}
                CurrentScreen::Error => {}
            },
            KeyCode::Esc => {
                match self.current_screen {
                    CurrentScreen::Main => {}
                    CurrentScreen::Editing => {
                        // if in EditMode return to EditScreen, if in EditScreen return to MainScreen
                        if self.currently_editing.is_some() {
                            self.edit_menu.select(0);
                            self.currently_editing = None;
                            self.clear_strings();
                        } else {
                            self.current_screen = CurrentScreen::Main;
                            self.edit_menu.deselect();
                            self.main_menu.select(1);
                        }
                    }
                    CurrentScreen::SoundSelection => {
                        self.switch_screen(CurrentScreen::Editing);
                    }
                    CurrentScreen::Exiting => {}
                    CurrentScreen::Error => {}
                }
            }
            _ => {}
        }
    }
    
    // Added these helper functions so app is in charge of its own atomics --------------------------------------
    pub fn get_bpm(&mut self) -> u64 {
        self.settings.bpm.load(Ordering::Relaxed)
    }
    pub fn get_volume(&mut self) -> f64 {
        self.settings.volume.load(Ordering::Relaxed)
    }
    pub fn get_is_running(&mut self) -> bool {
        self.settings.is_running.load(Ordering::Relaxed)
    }
    pub fn get_time_sig_string(&mut self) -> String {
        let note = self.settings.ts_note.load(Ordering::Relaxed).to_string();
        let value = self.settings.ts_value.load(Ordering::Relaxed).to_string();
        note + "/" + &value
    }
    pub fn get_bar_count_string(&mut self) -> String {
        self.settings.bar_count.load(Ordering::Relaxed).to_string()
    }
    pub fn get_selected_sound_string(&mut self) -> String {
        self.sound_list[self.settings.selected_sound.load(Ordering::Relaxed)].to_string()
    }
}

// Tests ---------------------------------------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SETTINGS: InitMetronomeSettings = InitMetronomeSettings {
        bpm: 120,
        ts_note: 4,
        ts_value: 4,
        volume: 100.0,
        is_running: false,
        debug: false,
    };

    const TEST_TICK_RATE: u64 = 7;

    // helper functions should return their values
    #[test]
    fn app_get_bpm() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        assert_eq!(test_app.get_bpm(), 120);
    }

    #[test]
    fn app_get_volume() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        assert_eq!(test_app.get_volume(), 100.0);
    }

    #[test]
    fn app_get_is_running() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        assert_eq!(test_app.get_is_running(), false);
    }

    // change functions should change the internal state of app based on edit_string
    #[test]
    fn app_change_bpm_editor() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "200".to_string();
        test_app.change_bpm_editor();
        assert_eq!(test_app.get_bpm(), 200);
    }

    // app::change_bpm should not change bpm with invalid input
    #[test]
    fn app_change_bpm_bad_input() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "hey this isn't a number is it?".to_string();
        assert_eq!(test_app.change_bpm_editor(), false);
        assert_eq!(test_app.get_bpm(), 120);
    }

    #[test]
    fn app_change_bpm_value_too_big() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "500000".to_string();
        assert_eq!(test_app.change_bpm_editor(), false);
        assert_eq!(test_app.get_bpm(), 120);
    }

    #[test]
    fn app_change_bpm_value_too_small() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "19".to_string();
        assert_eq!(test_app.change_bpm_editor(), false);
        assert_eq!(test_app.get_bpm(), 120);
    }

    #[test]
    fn app_change_bpm_value_negative() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "-120".to_string();
        assert_eq!(test_app.change_bpm_editor(), false);
        assert_eq!(test_app.get_bpm(), 120);
    }

    #[test]
    fn app_change_bpm_value_is_float() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "120.5".to_string();
        assert_eq!(test_app.change_bpm_editor(), false);
        assert_eq!(test_app.get_bpm(), 120);
    }

    // app::change_volume should not change volume with bad input
    #[test]
    fn app_change_volume_editor_bad_input() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "hey this isn't a number is it?".to_string();
        assert_eq!(test_app.change_volume_editor(), false);
        assert_eq!(test_app.get_volume(), 100.0);
    }

    #[test]
    fn app_change_volume_editor_value_too_big() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "500000".to_string();
        assert_eq!(test_app.change_volume_editor(), false);
        assert_eq!(test_app.get_volume(), 100.0);
    }

    #[test]
    fn app_change_volume_editor_value_too_small() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "0".to_string();
        assert_eq!(test_app.change_volume_editor(), false);
        assert_eq!(test_app.get_volume(), 100.0);
    }

    #[test]
    fn app_change_volume_editor_value_negative() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "-120".to_string();
        assert_eq!(test_app.change_volume_editor(), false);
        assert_eq!(test_app.get_volume(), 100.0);
    }

    // app::toggle_metronome should toggle metronome
    #[test]
    fn app_toggle_metronome() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        assert_eq!(test_app.get_is_running(), false);
        test_app.toggle_metronome();
        assert_eq!(test_app.get_is_running(), true);
        test_app.toggle_metronome();
        assert_eq!(test_app.get_is_running(), false);
    }

    // app::get_ns_from_bpm should correctly calculate the nanosecond offset from bpm
    #[test]
    fn app_get_ns_from_bpm() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        assert_eq!(test_app.get_ns_from_bpm(), 500_000_000);
    }

    // app::clear_strings should clear it's edit and notification strings when told to
    #[test]
    fn app_clear_strings() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        test_app.edit_string = "Don't forget a towel!".to_string();
        test_app.alert_string = "I mean it, don't forget a towel!".to_string();

        assert!(!test_app.edit_string.is_empty());
        assert!(!test_app.alert_string.is_empty());

        test_app.clear_strings();

        assert!(test_app.edit_string.is_empty());
        assert!(test_app.alert_string.is_empty());
    }

    // app::u64_in_range should correctly determine which values are in range
    #[test]
    fn app_u64_in_range() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        let min :u64 = 20;
        let max :u64 = 500;
        assert_eq!(test_app.u64_in_range(19, min, max), false);
        assert_eq!(test_app.u64_in_range(501, min, max), false);
        assert_eq!(test_app.u64_in_range(120, min, max), true);
        assert_eq!(test_app.u64_in_range(500, min, max), true);
        assert_eq!(test_app.u64_in_range(20, min, max), true);
    }

    // app::f64_in_range should correctly determine which values are in range
    #[test]
    fn app_f64_in_range() {
        let mut test_app = App::new(TEST_SETTINGS, TEST_TICK_RATE);
        let min :f64 = 1.0;
        let max :f64 = 200.0; 
        assert_eq!(test_app.f64_in_range(0.0, min, max), false);
        assert_eq!(test_app.f64_in_range(201.0, min, max), false);
        assert_eq!(test_app.f64_in_range(120.0, min, max), true);
        assert_eq!(test_app.f64_in_range(200.0, min, max), true);
        assert_eq!(test_app.f64_in_range(1.0, min, max), true);
    }
}
