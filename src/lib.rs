extern crate chrono;
#[macro_use]
extern crate crossbeam_channel as channel;
extern crate epoll;
#[macro_use]
extern crate nom;
#[cfg(target_os = "linux")]
extern crate alsa;
#[cfg(feature = "dbus")]
extern crate dbus;
extern crate libc;
#[cfg(target_os = "linux")]
extern crate libpulse_binding as pulse;
extern crate notify;
extern crate serde;
extern crate serde_json;
#[cfg(feature = "systemstat")]
extern crate systemstat;
#[cfg(feature = "xkb")]
extern crate xcb;
#[macro_use]
extern crate serde_derive;

pub mod format;
pub mod widget;

use std::{
    collections::BTreeMap,
    io::{BufRead, StdoutLock, Write},
};

pub use format::*;
pub use widget::*;

pub struct UnixBar<F: Formatter> {
    formatter: F,
    widgets: Vec<Box<dyn Widget>>,
    fns: BTreeMap<String, Box<dyn FnMut()>>,
}

impl<F: Formatter> UnixBar<F> {
    pub fn new(formatter: F) -> UnixBar<F> {
        UnixBar {
            formatter,
            widgets: Vec::new(),
            fns: BTreeMap::new(),
        }
    }

    pub fn register_fn<Fn>(&mut self, name: &str, func: Fn) -> &mut UnixBar<F>
    where
        Fn: FnMut() + 'static,
    {
        self.fns.insert(name.to_owned(), Box::new(func));
        self
    }

    pub fn add(&mut self, widget: Box<dyn Widget>) -> &mut UnixBar<F> {
        self.widgets.push(widget);
        self
    }

    pub fn run(&mut self) {
        let (wid_tx, wid_rx) = channel::unbounded();
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();

        for widget in &mut self.widgets {
            widget.spawn_notifier(wid_tx.clone());
        }
        self.show(&mut stdout);
        let (stdin_tx, stdin_rx) = channel::unbounded();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut stdin = stdin.lock();
            let mut line = String::new();
            loop {
                line.clear();
                if stdin.read_line(&mut line).is_ok() {
                    stdin_tx.send(line.clone()).unwrap();
                }
            }
        });
        loop {
            select! {
                recv(wid_rx) -> _ => self.show(&mut stdout),
                recv(stdin_rx) -> line => self.formatter.handle_stdin(line.ok(), &mut self.fns),
            }
        }
    }

    fn show(&mut self, stdout: &mut StdoutLock) {
        let vals: Vec<Format> = self.widgets.iter().map(|ref w| w.current_value()).collect();
        let line = self.formatter.format_all(&vals);
        let _ = writeln!(stdout, "{}", line.replace("\n", ""));
    }
}
