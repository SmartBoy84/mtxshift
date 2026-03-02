pub mod apps;
pub mod hardware;

use std::{
    sync::{Arc, Mutex},
    thread,
};

use crate::{
    apps::{MatrixApp, timer::blinky, shift::shift},
    hardware::{
        ButtonMonitor, ButtonMonitorFunctionality, Matrix, MatrixFunctionality, SharedDisplay,
    },
};

use rouille::Response;
use smol::{
    Executor,
    future::{self, FutureExt, block_on},
};

const DIN: usize = 2;
const CS: usize = 3;
const CLK: usize = 4;

const RIGHT_BUTTON: u32 = 27;
const LEFT_BUTTON: u32 = 17;

fn main() {
    let mut display = Matrix::new(DIN, CS, CLK).unwrap();

    display.clear_display(0).unwrap();
    display.set_intensity(0, 0x1).unwrap();

    let display = SharedDisplay::new(display);

    let server_display = display.clone();

    let _server = thread::spawn(|| {
        let state = Arc::new(Mutex::new(true));
        rouille::start_server("192.168.0.123:3141", move |h| {
            let mut display = smol::block_on(server_display.lock());
            let mut state = state.lock().unwrap();
            match (&h.url()[1..], *state) {
                ("on" | "toggle", false) => {
                    *state = true;
                    display.set_power(true).unwrap();
                }
                ("off" | "toggle", true) => {
                    *state = false;
                    display.set_power(false).unwrap();
                }
                (i, _) => {
                    if let Ok(i) = i.parse::<u8>() {
                        display.set_intensity(0, i).unwrap()
                    } else {
                        return Response::empty_404();
                    }
                }
            };
            Response::text("success!")
        })
    });

    // for tasks I want to continue in the background
    let background_runner = Arc::new(Executor::new());
    let new_back = background_runner.clone();
    thread::spawn(move || {
        smol::block_on(new_back.run(future::pending::<()>()));
    });

    // buttons
    let left_button = ButtonMonitor::new(LEFT_BUTTON);
    let left_button_events = left_button.get_recv();
    let right_button = ButtonMonitor::new(RIGHT_BUTTON);
    let right_button_events = right_button.get_recv();

    background_runner
        .spawn(async move {
            left_button.monitor().await;
        })
        .detach();
    
    background_runner
        .spawn(async move {
            right_button.monitor().await;
        })
        .detach();

    // apps
    let woolies = MatrixApp::new(shift, &display, &right_button_events);
    let pomodoro = MatrixApp::new(blinky, &display, &right_button_events);

    for app in [woolies, pomodoro].iter().cycle() {
        // clear it for next loop
        for _ in 0..left_button_events.len() {
            let _ = left_button_events.recv_blocking();
        }
        for _ in 0..right_button_events.len() {
            let _ = right_button_events.recv_blocking();
        }
        block_on(app.resume().or(async {
            loop {
                match left_button_events.recv().await {
                    Err(e) => {
                        println!("{e:?}");
                        continue;
                    }
                    _ => break,
                }
            }
            println!("next app!")
        }));
    }
}
