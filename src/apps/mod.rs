use std::sync::Arc;

use smol::{
    Executor,
    channel::{Receiver, Sender, TrySendError},
    future,
};

use crate::hardware::{Matrix, MatrixFunctionality, SharedDisplay};

pub mod timer;
pub mod shift;

pub struct MatrixApp<'a> {
    ex: Arc<Executor<'a>>,
    tx: Sender<()>,
}

impl<'a> MatrixApp<'a> {
    pub fn new<T, F, G, D>(f: G, d: &SharedDisplay<D>, b: &Receiver<()>) -> Self
    where
        T: Send + 'a,
        F: Future<Output = T> + Send + 'a,
        G: FnOnce(SharedDisplay<D>, Arc<Executor<'a>>, Receiver<()>, Receiver<()>) -> F,
        Matrix<D>: MatrixFunctionality,
    {
        let ex = Arc::new(Executor::new());
        let (tx, rx) = smol::channel::bounded::<()>(1);
        ex.spawn(f(d.clone(), ex.clone(), rx, b.clone())).detach();
        Self { ex, tx }
    }

    pub async fn resume(&self) {
        match self.tx.try_send(()) {
            Err(TrySendError::Full(_)) => (),
            e @ _ => e.unwrap(),
        };
        self.ex.run(future::pending::<()>()).await
    }
}
