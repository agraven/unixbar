use notify::{immediate_watcher, EventKind::Modify, RecommendedWatcher, RecursiveMode, Watcher};

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::{fs, io, thread};

use format::data::Format;
use widget::base::Sender;
use widget::Widget;

/// Gets the first directory in /sys/class/backlight
fn backlight_dir() -> Result<PathBuf, io::Error> {
    Ok(fs::read_dir(Path::new("/sys/class/backlight"))?
        .nth(0)
        .ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            "No backlight interface",
        ))??
        .path())
}

/// Reads a value from the kernel backlight interface
fn read_value(file: &str) -> Result<u32, io::Error> {
    let dir = backlight_dir()?;
    let string = fs::read_to_string(&dir.join(file))?;
    // NOTE: Assuming kernel always stores valid int in the file, something has gone horribly wrong
    // if it doesn't
    Ok(string.trim_end().parse().unwrap())
}

/// Writes a value to the kernel backlight interface
fn write_value(file: &str, value: u32) -> Result<(), io::Error> {
    let dir = backlight_dir()?;
    let mut fd = fs::File::create(&dir.join(file))?;
    write!(fd, "{}", value)
}

type F = dyn Fn() -> Format + Send + Sync + 'static;

/// Widget that gets and sets the screen's brightness value
pub struct Backlight {
    last_value: Arc<RwLock<Format>>,
    updater: Arc<Box<F>>,
    channel: (
        channel::Sender<notify::Event>,
        channel::Receiver<notify::Event>,
    ),
    watcher: RecommendedWatcher,
}

impl Backlight {
    pub fn new(updater: impl Fn() -> Format + Send + Sync + 'static) -> Box<Backlight> {
        let val = updater();
        let (tx, rx) = channel::unbounded();
        let tx_clone = tx.clone();
        let watcher = immediate_watcher(move |event: Result<notify::Event, notify::Error>| {
            if let Ok(event) = event {
                tx_clone.send(event).expect("channel error");
            }
        })
        .unwrap();
        Box::new(Backlight {
            last_value: Arc::new(RwLock::new(val)),
            updater: Arc::new(Box::new(updater)),
            channel: (tx, rx),
            watcher,
        })
    }

    /// Gets the current brightness value
    pub fn get() -> Result<f32, io::Error> {
        let brightness = read_value("brightness")?;
        let max = read_value("max_brightness")?;
        Ok(brightness as f32 / max as f32)
    }

    /// Sets the current brightness value on a scale from 0.0 to 1.0
    pub fn set(percent: f32) -> Result<(), io::Error> {
        let max = read_value("max_brightness")?;
        let new = (max as f32 * percent).round() as u32;
        write_value("brightness", new)
    }

    /// Adjusts the current brightness value by `percent` (should be from -1.0 to 1.0)
    pub fn adjust(percent: f32) -> Result<(), io::Error> {
        let brightness = read_value("brightness")?;
        let max = read_value("max_brightness")?;
        let new = brightness + (max as f32 * percent as f32) as u32;
        // Min is to prevent accidentally blackening the screen
        write_value("brightness", new.max(1))
    }
}

impl Widget for Backlight {
    fn current_value(&self) -> Format {
        (*self.last_value).read().unwrap().clone()
    }

    fn spawn_notifier(&mut self, tx: Sender<()>) {
        let updater = self.updater.clone();
        let last_value = self.last_value.clone();

        let rx2 = self.channel.1.clone();
        // set watcher to watch file with brightness value, return if non-existant.
        match backlight_dir().map(|dir| dir.join("brightness")) {
            Ok(dir) => self
                .watcher
                .watch(dir, RecursiveMode::NonRecursive)
                .unwrap(),
            _ => return,
        };

        thread::spawn(move || loop {
            match rx2.recv() {
                Ok(event) => match event.kind {
                    Modify(_) => {
                        tx.send(()).unwrap();
                        let mut writer = last_value.write().unwrap();
                        *writer = (*updater)();
                    }
                    _ => (),
                },
                _ => (),
            }
        });
    }
}
