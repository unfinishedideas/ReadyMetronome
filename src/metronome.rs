/// This file houses the Metronome code which has the audio event loop for running the click
/// It is started on a new thread by App and also shares state with it via Arc variables
use atomic_float::AtomicF64;
use color_eyre::{eyre::eyre, Report, Result};
use rodio::source::Source;
use rodio::{Decoder, OutputStream, OutputStreamHandle};
use std::{
    fs::File,
    io,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct Metronome {
    pub settings: MetronomeSettings,
}

// These settings are also shared with an instance of App to update the metronome after it has been
// moved to a new thread
//
// bpm                  : bpm for user interface
// ns_delay             : nanosecond delay between beats
// ts_note              : num of beats in a bar
// ts_value             : value of the beat (ie 1/4 notes (4) 1/8 notes (8) etc)
// ts_triplets          : set the metronome into triplet mode
// sub_eights           : subdivide the click into eighth notes
// sub_sixteens         : subdivide the click into sixteenth notes
// current_beat_count   : the current beat being played within the bar
// beats_per_bar        : number of beats played by the metronome per bar (ie. 6 beats in a 4/4 triplets bar)
// bar_count            : the number of bars elapsed since starting the metronome
// is_running           : whether or not the metronome is running
// volume               : volume of the metronome sound
// sound_list           : vector of strings of selectable sounds (from the /assets folder)
// selected_sound       : index in the sound_list of the selected sound
// tick_count           : the current tick count for the refresh rate
// debug                : enable debugging mode
// error                : used to report errors to the front end
//
pub struct MetronomeSettings {
    pub bpm: Arc<AtomicU64>,
    pub ns_delay: Arc<AtomicU64>,
    pub ts_note: Arc<AtomicU64>,
    pub ts_value: Arc<AtomicU64>,
    pub ts_triplets: Arc<AtomicBool>,
    pub sub_eights: Arc<AtomicBool>,
    pub sub_sixteens: Arc<AtomicBool>,
    pub current_beat_count: Arc<AtomicU64>,
    pub beats_per_bar: Arc<AtomicU64>,
    pub bar_count: Arc<AtomicU64>,
    pub is_running: Arc<AtomicBool>,
    pub volume: Arc<AtomicF64>,
    pub sound_list: Vec<String>,
    pub selected_sound: Arc<AtomicUsize>,
    pub tick_count: Arc<AtomicU64>,
    pub debug: Arc<AtomicBool>,
    pub error: Arc<AtomicBool>,
}

// This interface is used to set up the metronome without having to initialize internal variables
#[derive(Clone, Copy)]
pub struct InitMetronomeSettings {
    pub bpm: u64,
    pub ts_note: u64,
    pub ts_value: u64,
    pub volume: f64,
    pub debug: bool,
    pub is_running: bool,
}

impl Metronome {
    pub fn new(new_settings: &MetronomeSettings) -> Metronome {
        Metronome {
            settings: MetronomeSettings {
                bpm: Arc::clone(&new_settings.bpm),
                ns_delay: Arc::clone(&new_settings.ns_delay),
                ts_note: Arc::clone(&new_settings.ts_note),
                ts_value: Arc::clone(&new_settings.ts_value),
                ts_triplets: Arc::clone(&new_settings.ts_triplets),
                sub_eights: Arc::clone(&new_settings.sub_eights),
                sub_sixteens: Arc::clone(&new_settings.sub_sixteens),
                current_beat_count: Arc::clone(&new_settings.current_beat_count),
                beats_per_bar: Arc::clone(&new_settings.beats_per_bar),
                bar_count: Arc::clone(&new_settings.bar_count),
                is_running: Arc::clone(&new_settings.is_running),
                volume: Arc::clone(&new_settings.volume),
                sound_list: new_settings.sound_list.clone(),
                selected_sound: Arc::clone(&new_settings.selected_sound),
                tick_count: Arc::clone(&new_settings.tick_count),
                debug: Arc::clone(&new_settings.debug),
                error: Arc::clone(&new_settings.error),
            },
        }
    }

    pub fn start(&mut self, refresh_rate: u64) {
        let refresh_rate = Duration::from_nanos(refresh_rate);
        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
        let mut running = self.settings.is_running.load(Ordering::Relaxed);
        let mut last_refresh = Instant::now();

        // Metronome first / last tick used for timing beats
        let mut first_tick = true;
        let mut last_tick = Instant::now();

        loop {
            let timeout_refresh = refresh_rate
                .checked_sub(last_refresh.elapsed())
                .unwrap_or(refresh_rate);

            if running {
                // Exit the loop if there was an error
                if self.settings.error.load(Ordering::Relaxed) {
                    return;
                }
                // Run the first tick if the metronome was just started
                if first_tick {
                    first_tick = false;
                    self.start_tick_thread(stream_handle.clone());
                    last_tick = Instant::now();
                } else {
                    let time_since_last_tick = Instant::now().duration_since(last_tick);
                    let delay =
                        Duration::from_nanos(self.settings.ns_delay.load(Ordering::Relaxed));
                    if time_since_last_tick >= delay {
                        last_tick = Instant::now();
                        self.start_tick_thread(stream_handle.clone());
                    }
                }
            }

            running = self.settings.is_running.load(Ordering::Relaxed);
            if !running {
                self.settings.bar_count.swap(1, Ordering::Relaxed);
                self.settings.current_beat_count.swap(0, Ordering::Relaxed);
                first_tick = true;
            }
            // We always sleep for the tick duration regardless if the metronome is running
            spin_sleep::sleep(timeout_refresh);

            // Perform debug functionality
            if self.settings.debug.load(Ordering::Relaxed) && last_refresh.elapsed() >= refresh_rate
            {
                let current_tick_count = self.settings.tick_count.load(Ordering::Relaxed);
                let result = current_tick_count.checked_add(1).unwrap_or(0);
                self.settings.tick_count.swap(result, Ordering::Relaxed);
                last_refresh = Instant::now();
            }
        }
    }

    // Load the tick function into a new thread for execution (that way this isn't tied to bpm anymore)
    fn start_tick_thread(&mut self, stream_handle: OutputStreamHandle) {
        let selected_sound_name =
            self.settings.sound_list[self.settings.selected_sound.load(Ordering::Relaxed)].clone();
        let volume = self.settings.volume.load(Ordering::Relaxed);
        let error = self.settings.error.clone();
        let handler = thread::spawn(move || {
            match metronome_tick(stream_handle, selected_sound_name, volume) {
                Ok(_) => {}
                Err(_) => {
                    error.swap(true, Ordering::Relaxed);
                }
            }
        });
        // close the thread to prevent multiples from spawning
        let _ = handler.join();
        self.beat_count();
    }

    // Counts the number of beats and updates bar_count
    fn beat_count(&mut self) {
        let mut current_beat_count = self.settings.current_beat_count.load(Ordering::Relaxed);
        if current_beat_count == self.settings.beats_per_bar.load(Ordering::Relaxed) {
            self.settings.current_beat_count.swap(1, Ordering::Relaxed);
            let new_bar_count = self.settings.bar_count.load(Ordering::Relaxed) + 1;
            self.settings
                .bar_count
                .swap(new_bar_count, Ordering::Relaxed);
        } else {
            current_beat_count += 1;
            self.settings
                .current_beat_count
                .swap(current_beat_count, Ordering::Relaxed);
        }
    }
}

fn metronome_tick(
    stream_handle: OutputStreamHandle,
    selected_sound_name: String,
    volume: f64,
) -> Result<(), Report> {
    // TODO: Don't load the sample every time, if possible load once and replay.
    let file = io::BufReader::new(
        match File::open("./assets/".to_owned() + &selected_sound_name) {
            Ok(value) => value,
            Err(_) => {
                return Err(eyre!("Error: Problem loading sound"));
            }
        },
    );

    let source = Decoder::new(file).unwrap();
    let _ = stream_handle.play_raw(source.amplify((volume / 100.0) as f32).convert_samples());
    Ok(())
}
