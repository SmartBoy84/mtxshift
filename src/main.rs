pub mod apps;
pub mod hardware;

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crate::{
    apps::{MatrixApp, shift::shift, timer::blinky},
    hardware::{
        ButtonMonitor, ButtonMonitorFunctionality, DEBOUNCE_DUR, Matrix, MatrixFunctionality,
        SharedDisplay,
    },
};

use futures::{
    FutureExt,
    future::{self, Either},
    select,
};
use rouille::Response;
use smol::{
    Executor, Timer, channel,
    future::{FutureExt as SmolFutureExt, block_on},
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
            for app in [woolies, pomodoro].iter().cycle() {
                // clear it for next loop
                while left_button_events.try_recv().is_ok() {}
                while right_button_events.try_recv().is_ok() {}

                app.resume(&right_button_events)
                    .or(async {
                        let left_button_events = left_button_events.clone();
                            const QUICK_PRESS_WAIT: Duration = Duration::from_secs(2); // max 1 second delay
                            assert!(QUICK_PRESS_WAIT > 5 * DEBOUNCE_DUR); // just to be safe
                            // have to press twice - hack to fix phantom presses
                            let mut i = 2;
                            loop {
                                select! {
                                    o = left_button_events.recv().fuse() => {
                                        match o {
                                            Err(e) => {
                                                println!("{e:?}");
                                                continue;
                                            }
                                            _ => {
                                            i -= 1;
                                            if i == 0 {
                                                break
                                            } else {
                                                continue
                                            }
                                            }
                                        }
                                    }
                                    _ = if i == 1 {
                                        Either::Right(async {Timer::after(QUICK_PRESS_WAIT).await;}).fuse()
                                    } else {
                                          // don't do any timeout shit, if button hasn't been pressed once
                                          Either::Left(future::pending::<()>()).fuse()
                                            
                                    } => {
                                        i = 2; // maaan the nesting is so fuckiing deep it ain't auto formatting anymore...
                                        continue
                                    }

                                };
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
