use pulse::{
    callbacks::ListResult,
    context::{subscribe::subscription_masks, Context},
    mainloop::standard::{IterateResult, Mainloop},
    volume::VOLUME_NORM,
};

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, RwLock},
    thread,
};

use super::{VolumeBackend, VolumeState};
use crate::{format::data::Format, widget::base::Sender};

pub struct Pulse {
    last_value: Arc<RwLock<Format>>,
}

impl Pulse {
    pub fn new() -> Pulse {
        Pulse {
            last_value: Arc::new(RwLock::new(Format::Str(String::new()))),
        }
    }
}

impl<F> VolumeBackend<F> for Pulse
where
    F: Fn(VolumeState) -> Format + Send + Sync + 'static,
{
    fn current_value(&self) -> Format {
        (*self.last_value).read().unwrap().clone()
    }

    fn spawn_notifier(&mut self, tx: Sender<()>, updater: Arc<Box<F>>) {
        let last_value = self.last_value.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            let mut mainloop = Mainloop::new().unwrap();
            let context = Rc::new(RefCell::new(Context::new(&mainloop, "bars").unwrap()));
            context
                .borrow_mut()
                .connect(None, pulse::context::flags::NOFLAGS, None)
                .unwrap();

            // Wait for initialization, return if error occurs
            loop {
                match mainloop.iterate(false) {
                    IterateResult::Quit(_) | IterateResult::Err(_) => return,
                    IterateResult::Success(_) => {}
                }
                match context.borrow().get_state() {
                    pulse::context::State::Ready => break,
                    pulse::context::State::Failed | pulse::context::State::Terminated => return,
                    _ => {}
                }
            }

            // Subscribe to sink (audio output) events
            context
                .borrow_mut()
                .subscribe(subscription_masks::SINK, |_| {});

            // Set callback for events
            let context_clone = context.clone();
            context
                .borrow_mut()
                .set_subscribe_callback(Some(Box::new(move |_, _, _| {
                    let introspect = Rc::new(RefCell::new(context_clone.borrow().introspect()));
                    let introspect_clone = introspect.clone();

                    let last_value = last_value.clone();
                    let updater = updater.clone();
                    let tx = tx.clone();
                    // Get ServerInfo to get default sink name
                    introspect.borrow_mut().get_server_info(move |info| {
                        let last_value = last_value.clone();
                        let updater = updater.clone();
                        let tx = tx.clone();
                        introspect_clone.borrow().get_sink_info_by_name(
                            info.default_sink_name.as_ref().unwrap(),
                            move |sink| {
                                // Get volume of sink and run the updater on it
                                if let ListResult::Item(sink) = sink {
                                    let volume = sink.volume.max();
                                    // Convert volume to fraction with formula v/n + 1/200
                                    let value = volume.0 as f32 / VOLUME_NORM.0 as f32 + 0.005;

                                    let mut writer = last_value.write().unwrap();
                                    *writer = (*updater)(VolumeState {
                                        volume: value,
                                        muted: sink.mute,
                                    });
                                    tx.send(()).unwrap();
                                }
                            },
                        );
                    });
                })));

            let _ = mainloop.run();
        });
    }
}
