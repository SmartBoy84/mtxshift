pub mod apps;
pub mod hardware;

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crate::{
    apps::{MatrixApp, MatrixAppType, counter::counter, shift::shift, timer::timer},
    hardware::{
        ButtonMonitor, ButtonMonitorFunctionality, DEBOUNCE_DUR, Matrix, MatrixFunctionality,
        SharedDisplay,
    },
};

use futures::{
    FutureExt, TryFutureExt, future::{self, Either}, select
};
use rouille::Response;
use smol::{
    Executor, Timer, block_on, channel::{self, TrySendError}, future::FutureExt as SmolFutureExt
};

const DIN: usize = 2;
const CS: usize = 3;
const CLK: usize = 4;

const RIGHT_BUTTON: u32 = 27;
const LEFT_BUTTON: u32 = 17;

const SERVER_ADDR: &str = "192.168.0.123:3141";

pub enum Intensity {
    Pause(bool),
    Set(u8)
}

fn main() {
    let mut display = Matrix::new(DIN, CS, CLK).unwrap();
    let intensity = 0x1;

    display.clear_display(0).unwrap();
    display.set_power(true).unwrap();
    display.set_intensity(0, intensity).unwrap();

    let display = SharedDisplay::new(display);

    let (state_tx, state_rx) = channel::unbounded::<Intensity>();

    let state = Arc::new(Mutex::new(true));
    {
    let state_tx = state_tx.clone();
    let state = state.clone();
    let display = display.clone();
    
    let _server = thread::spawn(move || {

        rouille::start_server(SERVER_ADDR, move |h| {
            let state = state.lock().unwrap();

            match (&h.url()[1..], *state) {
                ("pause" | "toggle", true) => {
                    state_tx.send_blocking(Intensity::Pause(true)).unwrap();
                }
                ("unpause" | "toggle", false) => {
                    state_tx.send_blocking(Intensity::Pause(false)).unwrap();
                }
                ("display_off", p) => {
                    if p {
                        state_tx.send_blocking(Intensity::Pause(true)).unwrap(); // pause if not
                    }
                    smol::block_on(display.lock()).set_power(false).unwrap();
                }
                ("display_on", p) => {
                    if !p {
                        state_tx.send_blocking(Intensity::Pause(false)).unwrap();
                    }
                    smol::block_on(display.lock()).set_power(true).unwrap();
                }
                (i, _) => {
                    if let Ok(i) = i.parse::<u8>() {
                        state_tx.send_blocking(Intensity::Set(i)).unwrap();
                    } else {
                        return Response::empty_404();
                    }
                }
            };
            Response::text("success!")
        })
    });
    }

    // for tasks I want to continue in the background
    let app_runner = Arc::new(Executor::new());

    // buttons
    let left_button = ButtonMonitor::new(LEFT_BUTTON);
    let left_button_events = left_button.get_recv();
    let right_button = ButtonMonitor::new(RIGHT_BUTTON);
    let right_button_events = right_button.get_recv();

    // button monitor requires separate executor because block_on
        smol::
            spawn(async move {
                left_button.monitor().await;
            })
            .detach();

        smol::
            spawn(async move {
                right_button.monitor().await;
            })
            .detach();
    

    // apps
    let woolies = MatrixApp::new(MatrixAppType::NoPause(Box::new(shift)), &display, state_tx.clone());
    let pomodoro = MatrixApp::new(MatrixAppType::WithPause(Box::new(timer)), &display, state_tx.clone());
    let counter = MatrixApp::new(MatrixAppType::NoPause(Box::new(counter)), &display, state_tx.clone());
    // let test = MatrixApp::new(test::test, &display);

    let app_list = Arc::new([woolies, pomodoro, counter]); // to prevent dropping
    let current_app = Arc::new(smol::lock::Mutex::new(0usize));

    // can't share the button channel directly because i suspend the task -> the source may try to wake the dead task, instead of delivering message to my main block_on
    let (interface_tx, interface_rx) = smol::channel::bounded::<()>(1);

    // main app loop
    {
    let right_button_events = interface_rx;
    let left_button_events = left_button_events.clone();
    let app_list = app_list.clone();
    let current_app = current_app.clone();
    app_runner
        .spawn(async move {

            // keep track of first_run to prevent uncessary firing of refresh signal
            let mut app_cycle = (0..app_list.len()).map(|i| (i, true)).chain((0..app_list.len()).cycle().map(|i| (i, false))); 
            loop {
                let (i, first_run) = app_cycle.next().unwrap();
                *current_app.lock().await = i;

                // clear it for next loop
                while left_button_events.try_recv().is_ok() {}
                while right_button_events.try_recv().is_ok() {}

                app_list[i].resume(&right_button_events, first_run)
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
    }

    // block local executor on running app executor, and waiting for button press
    block_on(async move {
        let intensity = 0x1;
        display.lock().await.set_intensity(0, intensity).unwrap();
        loop {
            println!("starting up runner");
            let app_runner = app_runner.clone();
            let app_runner_task = smol::spawn(async move {app_runner.run(future::pending::<()>()).fuse().await; unreachable!("app runner crashed?")});
            
            let button_relay_task = {
                let right_button_events = right_button_events.clone();
                let interface_tx = interface_tx.clone();
                smol::spawn(async move {
                    loop {
                        match right_button_events.recv().map_ok(|_| interface_tx.try_send(())).await {
                            Ok(Ok(_)) => (), // rx and tx all good
                            e @ Err(_) => {let _ = e.unwrap();}
                            Ok(Err(TrySendError::Full(_))) => (),
                            Ok( e @ Err(_)) => {e.unwrap();},
                            }
                        }
                    })
            };

                    loop {
                        let o = match state_rx.recv().await {
                            Err(e) => {
                                println!("{e:?}");
                                continue;
                            },
                            Ok(o) => o
                        };

                        match o {
                            Intensity::Pause(true) => break,
                            Intensity::Set(i) => display.lock().await.set_intensity(0, i).unwrap(),
                            _ => continue
                        }
                    }

            println!("pausing app");
            let i = current_app.lock().await; // capture mutex to prevent app runner from going ahead

            let app = &app_list[*i];
            app.pause().await;
            *state.lock().unwrap() = false;

            println!("outer: app paused!");
        
            drop(app_runner_task); // now that app has been paused, stop the app executor
            drop(i); // safe to drop since app_runner isn't running
            drop(button_relay_task); // drop this so that button presses aren't redirected to a suspended task
            
            display.lock().await.set_intensity(0, 0).unwrap();

            loop {
                match state_rx.recv().or(right_button_events.recv().map_ok(|_|{ Intensity::Pause(false)})).await {
                    Ok(Intensity::Pause(false)) => break,
                    e => {
                        e.unwrap();
                    }
                }
            }
            display.lock().await.set_intensity(0, intensity).unwrap();
            
            println!("resuming app");
            app.unpause().await; // next loop iteration app executor starts back up as well, and it starts running again
            display.lock().await.set_power(true).unwrap();
            *state.lock().unwrap() = true;

            // finally, clear any button events to prevent unexpected app events
            while left_button_events.try_recv().is_ok() {}
            while right_button_events.try_recv().is_ok() {}
        }
    })
}
