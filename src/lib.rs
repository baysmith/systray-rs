// Systray Lib

#[macro_use]
extern crate log;

#[cfg(target_os = "linux")]

extern crate glib;
#[cfg(target_os = "linux")]
extern crate gtk;
#[cfg(target_os = "linux")]
#[cfg(target_os = "windows")]
extern crate libc;
#[cfg(target_os = "windows")]
extern crate winapi;
pub mod api;

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, Debug)]
pub enum SystrayError {
    OsError(String),
    NotImplementedError,
    UnknownError,
}

pub struct SystrayEvent {
    menu_id: u64,
    item_id: u32,
}

impl std::fmt::Display for SystrayError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            SystrayError::OsError(ref err_str) => write!(f, "OsError: {}", err_str),
            SystrayError::NotImplementedError => write!(f, "Functionality is not implemented yet"),
            SystrayError::UnknownError => write!(f, "Unknown error occurrred"),
        }
    }
}

#[derive (Default)]
pub struct MenuData {
    size: u32,
    callbacks: HashMap<u32, Callback>,
}

impl MenuData {
    pub fn new() -> Self {
        Default::default()
    }
}

pub struct Application {
    window: api::api::Window,
    menu_idx: u32,
    menu_data: HashMap<u64, MenuData>,
    // Each platform-specific window module will set up its own thread for
    // dealing with the OS main loop. Use this channel for receiving events from
    // that thread.
    rx: Receiver<SystrayEvent>,
}

type Callback = Box<(Fn(&mut Application) -> () + 'static)>;

fn make_callback<F>(f: F) -> Callback
where
    F: std::ops::Fn(&mut Application) -> () + 'static,
{
    Box::new(f) as Callback
}

impl Application {
    pub fn new() -> Result<Application, SystrayError> {
        let (event_tx, event_rx) = channel();
        let mut menu_data = HashMap::new();
        menu_data.insert(0, MenuData::new());
        match api::api::Window::new(event_tx) {
            Ok(w) => Ok(Application {
                window: w,
                menu_idx: 0,
                menu_data,
                rx: event_rx,
            }),
            Err(e) => Err(e),
        }
    }

    pub fn add_menu_group(&mut self, submenu: u64, item_name: &str) -> Result<u64, SystrayError> {
        if !self.menu_data.contains_key(&submenu) {
            return Ok(0);
        }
        let idx = self.menu_data.get(&submenu).unwrap().size;
        let subsubmenu = self
            .window
            .add_submenu_group(submenu, self.menu_idx, idx, item_name)?;
        self.menu_data.insert(subsubmenu, MenuData::new());
        self.menu_data.get_mut(&submenu).unwrap().size += 1;
        self.menu_idx += 1;
        Ok(subsubmenu)
    }

    pub fn add_menu_item<F>(
        &mut self,
        submenu: u64,
        item_name: &str,
        f: F,
    ) -> Result<u32, SystrayError>
    where
        F: std::ops::Fn(&mut Application) -> () + 'static,
    {
        if !self.menu_data.contains_key(&submenu) {
            return Ok(0);
        }
        let idx = self.menu_data.get(&submenu).unwrap().size;
        self.window
            .add_submenu_entry(submenu, self.menu_idx, idx, item_name)?;
        self.menu_data
            .get_mut(&submenu)
            .unwrap()
            .callbacks
            .insert(self.menu_idx, make_callback(f));
        self.menu_data.get_mut(&submenu).unwrap().size += 1;
        self.menu_idx += 1;
        Ok(idx)
    }

    pub fn add_menu_separator(&mut self, submenu: u64) -> Result<u32, SystrayError> {
        if !self.menu_data.contains_key(&submenu) {
            return Ok(0);
        }
        let idx = self.menu_data.get(&submenu).unwrap().size;
        if let Err(e) = self.window.add_menu_separator(self.menu_idx, idx) {
            return Err(e);
        }
        self.menu_data.get_mut(&submenu).unwrap().size += 1;
        self.menu_idx += 1;
        Ok(idx)
    }

    pub fn set_icon_from_file(&self, file: &str) -> Result<(), SystrayError> {
        self.window.set_icon_from_file(file)
    }

    pub fn set_icon_from_resource(&self, resource: &str) -> Result<(), SystrayError> {
        self.window.set_icon_from_resource(resource)
    }

    pub fn shutdown(&self) -> Result<(), SystrayError> {
        self.window.shutdown()
    }

    pub fn set_tooltip(&self, tooltip: &str) -> Result<(), SystrayError> {
        self.window.set_tooltip(tooltip)
    }

    pub fn quit(&mut self) {
        self.window.quit()
    }

    pub fn wait_for_message(&mut self) {
        loop {
            let msg;
            match self.rx.recv() {
                Ok(m) => msg = m,
                Err(_) => {
                    self.quit();
                    break;
                }
            }
            if self.menu_data.contains_key(&msg.menu_id)
                && self
                    .menu_data
                    .get(&msg.menu_id)
                    .unwrap()
                    .callbacks
                    .contains_key(&msg.item_id)
            {
                let f = self
                    .menu_data
                    .get_mut(&msg.menu_id)
                    .unwrap()
                    .callbacks
                    .remove(&msg.item_id)
                    .unwrap();
                f(self);
                self.menu_data
                    .get_mut(&msg.menu_id)
                    .unwrap()
                    .callbacks
                    .insert(msg.item_id, f);
            }
        }
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        self.shutdown().ok();
    }
}
