pub mod apps;
pub mod hardware;

use std::{
    sync::{Arc, Mutex},
    thread,
};

use crate::{
    apps::{MatrixApp, shift::shift, timer::blinky},
    hardware::{
        ButtonMonitor, ButtonMonitorFunctionality, Matrix, MatrixFunctionality, SharedDisplay,
    },
};

use futures::{FutureExt, select};
use rouille::Response;
use smol::{
    Executor, channel,
    future::{self, FutureExt as SmolFutureExt, block_on},
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

    let (state_tx, state_rx) = channel::unbounded::<bool>();

    let _server = thread::spawn(|| {
        let state = Arc::new(Mutex::new(true));
        rouille::start_server("192.168.0.123:3141", move |h| {
            let mut display = smol::block_on(server_display.lock());
            let mut state = state.lock().unwrap();
            match (&h.url()[1..], *state) {
                ("on" | "toggle", false) => {
                    *state = true;
                    state_tx.send_blocking(true).unwrap();
                    // display.set_power(true).unwrap();
                }
                ("off" | "toggle", true) => {
                    *state = false;
                    state_tx.send_blocking(false).unwrap();
                    // display.set_power(false).unwrap();
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
    let app_runner = Executor::new();

    // buttons
    let left_button = ButtonMonitor::new(LEFT_BUTTON);
    let left_button_events = left_button.get_recv();
    let right_button = ButtonMonitor::new(RIGHT_BUTTON);
    let right_button_events = right_button.get_recv();

    app_runner
        .spawn(async move {
            left_button.monitor().await;
        })
        .detach();

    app_runner
        .spawn(async move {
            right_button.monitor().await;
        })
        .detach();

    // apps
    let woolies = MatrixApp::new(shift, &display);
    let pomodoro = MatrixApp::new(blinky, &display);

    // main app loop
    app_runner
        .spawn(async move {
            for app in [pomodoro, woolies].iter().cycle() {
                // clear it for next loop
                while left_button_events.try_recv().is_ok() {}
                while right_button_events.try_recv().is_ok() {}

                app.resume(&right_button_events)
                    .or(async {
                        loop {
                            match left_button_events.clone().recv().await {
                                Err(e) => {
                                    println!("{e:?}");
                                    continue;
                                }
                                _ => break,
                            }
                        }
                        println!("next app!")
                    })
                    .await;
            }
        })
        .detach();

    // block local executor on running app executor, and waiting for button press
    block_on(async {
        loop {
            select! {
                () = app_runner.run(future::pending::<()>()).fuse() => todo!(),
                r = state_rx.recv().fuse() => match r {
                    Ok(false) => (),
                    e => {
                        e.unwrap();
                        continue;
                    }
                }
            };
            println!("pausing app");
            loop {
                match state_rx.recv().await {
                    Ok(true) => break,
                    e => {
                        e.unwrap();
                    }
                }
            }
            println!("resuming app")
        }
    })
}
