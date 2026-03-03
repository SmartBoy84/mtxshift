use std::sync::Arc;

use smol::{
    Executor,
    channel::{Receiver, Sender, TrySendError},
    future::{self, FutureExt},
};

use crate::hardware::{Matrix, MatrixFunctionality, SharedDisplay};

pub mod shift;
pub mod timer;

pub struct MatrixApp<'a> {
    ex: Arc<Executor<'a>>,
    refresh_tx: Sender<()>,
    button_tx: Sender<()>,
}

impl<'a> MatrixApp<'a> {
    pub fn new<T, F, G, D>(f: G, d: &SharedDisplay<D>) -> Self
    where
        T: Send + 'a,
        F: Future<Output = T> + Send + 'a,
        G: FnOnce(SharedDisplay<D>, Arc<Executor<'a>>, Receiver<()>, Receiver<()>) -> F,
        Matrix<D>: MatrixFunctionality,
    {
        let ex = Arc::new(Executor::new());
        let (refresh_tx, refresh_rx) = smol::channel::bounded::<()>(1);
        let (button_tx, button_rx) = smol::channel::bounded::<()>(1);
        ex.spawn(f(d.clone(), ex.clone(), refresh_rx, button_rx))
            .detach();
        Self {
            ex,
            refresh_tx,
            button_tx,
        }
    }

    pub async fn resume(&self, b: &Receiver<()>) {
        match self.refresh_tx.try_send(()) {
            Err(TrySendError::Full(_)) => (),
            e @ _ => e.unwrap(),
        };
        self.ex
            .run(future::pending::<()>())
            .or(async {
                loop {
                    /*
                    For some reason, sharing b to the actual app does not reliably let either app recv()
                    So instead, I listen to the button press event here, and signal button press via a channel specific to this app
                    That way the button receiver channel is only ever being listened to by one task/future
                     */
                    b.recv().await.unwrap();
                    match self.button_tx.try_send(()) {
                        Err(TrySendError::Full(_)) => (),
                        e @ _ => e.unwrap(),
                    };
                }
            })
            .await
    }
}
