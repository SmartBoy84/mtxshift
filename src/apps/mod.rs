use std::sync::Arc;

use futures::future::Either;
use smol::{
    Executor,
    channel::{Receiver, Sender, TrySendError},
    future::{self, FutureExt},
};

use crate::{
    Intensity,
    hardware::{Matrix, MatrixFunctionality, SharedDisplay},
};

// apps
pub mod counter;
pub mod shift;
pub mod timer;

pub struct MatrixApp<'a> {
    ex: Arc<Executor<'a>>,
    refresh_tx: Sender<()>,
    button_tx: Sender<()>,
    pause_tx: Sender<PauseType>,

    // store rx and tx so that PauseTracker can be instantiated
    pause_tx_ret: Sender<()>,
    pause_rx_ret: Receiver<()>,
}

pub enum PauseType {
    Pause(PauseTracker),
    Unpause,
}

/// MAKE sure to pull in the PauseTracker when doing .recv().await else external users assume you have paused immediately - will not wait till actually paused!
pub struct PauseTracker(Sender<()>); // smol::channel is multiple tx/rx - so can indicate pause on same channel

impl PauseTracker {
    fn new(tx: Sender<()>) -> Self {
        Self(tx)
    }
}

impl Drop for PauseTracker {
    fn drop(&mut self) {
        self.0.send_blocking(()).unwrap();
        // pretty nifty, the user waits for pause, does whatever it needs to get ready to pause
        // then once it's pausing shizzle is done, this gets dropped and it is automatically indicated
        // all without stopping the actual future running the task!
    }
}

// Box used here to erase concrete types, else user would need to specify Fn1 AND Fn2 even though only Fn1 used
// Overhead is fine since I only use it once in initialisation to get the function
pub enum MatrixAppType<'a, D, T: Send + 'a, F: Future<Output = T> + Send + 'a> {
    WithPause(
        Box<
            dyn FnOnce(
                SharedDisplay<D>,    // matrix display
                Arc<Executor<'a>>,   // executor that will be running this app
                Receiver<()>,        // app refresh signal
                Receiver<()>,        // app interface button signal
                Receiver<PauseType>, // global pause signal
                Sender<Intensity>,   // global pause trigger -> allows app to trigger a global pause
            ) -> F,
        >,
    ),
    NoPause(
        Box<
            dyn FnOnce(
                SharedDisplay<D>,
                Arc<Executor<'a>>,
                Receiver<()>,
                Receiver<()>,
                Sender<Intensity>,
            ) -> F,
        >,
    ),
}

impl<'a> MatrixApp<'a> {
    // app can check for when app has been refreshed/reloaded, interface button has been pressed, or app is paused
    pub fn new<T, F, D>(
        f: MatrixAppType<'a, D, T, F>,
        d: &SharedDisplay<D>,
        g_p: Sender<Intensity>,
    ) -> Self
    where
        T: Send + 'a,
        F: Future<Output = T> + Send + 'a,
        Matrix<D>: MatrixFunctionality,
    {
        let ex = Arc::new(Executor::new());
        let (refresh_tx, refresh_rx) = smol::channel::bounded::<()>(1);
        let (button_tx, button_rx) = smol::channel::bounded::<()>(1);
        let (pause_tx, pause_rx) = smol::channel::bounded::<PauseType>(1);
        let (pause_tx_ret, pause_rx_ret) = smol::channel::bounded::<()>(1);

        ex.spawn(match f {
            // this match ensures that pause_rx is dropped if function doesn't need it
            MatrixAppType::WithPause(f) => f(
                d.clone(),
                ex.clone(),
                refresh_rx,
                button_rx,
                pause_rx,
                g_p.clone(),
            ),
            MatrixAppType::NoPause(f) => {
                pause_rx.close();
                drop(pause_rx); // it would likely have been automatically done but just make it explicit

                f(d.clone(), ex.clone(), refresh_rx, button_rx, g_p.clone())
            }
        })
        .detach();

        Self {
            ex,
            refresh_tx,
            button_tx,
            pause_tx,
            pause_rx_ret,
            pause_tx_ret,
        }
    }

    pub async fn pause(&self) {
        if self.pause_tx.is_closed() {
            return;
        }
        // signal pause
        self.pause_tx
            .force_send(PauseType::Pause(PauseTracker::new(
                self.pause_tx_ret.clone(),
            )))
            .unwrap(); // force send, because if it was told to unpause, it's too late to handle that now

        println!("waiting...");
        // wait for pause tracker to be dropped
        match self.pause_rx_ret.recv().await {
            Ok(_) => (),
            e @ Err(_) => {
                e.unwrap();
            }
        }

        // NOTE; i do not also signal unpause on the same channel by transmitting None because that might cause bugs later -> multiple users of matrixapp, one in pause, other uses unpause -> pause thinks the app sent None but it was an unpause request...
    }

    pub async fn unpause(&self) {
        if self.pause_tx.is_closed() {
            return;
        }

        // force send in case it forgot to handle previous pause request -> too late for that new, resume
        self.pause_tx.force_send(PauseType::Unpause).unwrap();
    }

    pub async fn resume(&self, b: &Receiver<()>, first_run: bool) {
        if !first_run {
            match self.refresh_tx.try_send(()) {
                Err(TrySendError::Full(_) | TrySendError::Closed(_)) => (), // closed is not an error -> listener may not care to know when an app refresh has happened
                e @ _ => e.unwrap(),
            };
        }
        self.ex
            .run(future::pending::<()>())
            .or(if self.button_tx.is_closed() {
                Either::Right(future::pending::<()>())
            } else {
                println!("somehow got button");
                // use if statement here, rathwer than check closed case in match to prevent uncessary waiting on the b receiver
                Either::Left(async {
                    // infinite loop? nuh uh, don't be alarmed, this whole thing is a future that i return
                    // if the user drops this future, the loop is dropped as well
                    loop {
                        /*
                        For some reason, sharing b to the actual app does not reliably let either app recv()
                        So instead, I listen to the button press event here, and signal button press via a channel specific to this app
                        That way the button receiver channel is only ever being listened to by one task/future
                         */
                        b.recv().await.unwrap();
                        match self.button_tx.try_send(()) {
                            Err(TrySendError::Full(_)) => (), // don't need to account for closed case
                            e @ _ => e.unwrap(),
                        };
                    }
                })
            })
            .await
    }
}
