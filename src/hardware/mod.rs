#[cfg(target_os = "macos")]
pub mod mac_dummy;

#[cfg(target_os = "linux")]
pub mod rpi;

use std::{fmt::Debug, sync::Arc, time::Duration};

use smol::{
    channel::{self, Receiver, Sender, TrySendError},
    lock::{Mutex, futures::Lock},
};

pub struct SharedDisplay<T>(Arc<Mutex<Matrix<T>>>);

impl<T> Clone for SharedDisplay<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct Matrix<T>(T);

impl<T> SharedDisplay<T>
where
    Matrix<T>: MatrixFunctionality,
{
    pub fn new(m: Matrix<T>) -> Self {
        Self(Arc::new(Mutex::new(m)))
    }

    pub fn lock<'a>(&self) -> Lock<'_, Matrix<T>> {
        self.0.lock()
    }
}

const DEBOUNCE_DUR: Duration = Duration::from_millis(250); // max millis in between press to count as legitimate press

pub struct ButtonMonitor {
    tx: Sender<()>,
    rx: Receiver<()>,
    line: u32,
}

impl ButtonMonitor {
    pub fn new(line: u32) -> Self {
        let (tx, rx) = channel::bounded::<()>(1);
        Self { tx, rx, line }
    }
    pub fn get_recv(&self) -> Receiver<()> {
        self.rx.clone()
    }
}

pub trait ButtonMonitorFunctionality {
    fn monitor(&self) -> impl Future;
}

pub trait MatrixFunctionality {
    type Err: Debug;
    fn new(din: usize, cs: usize, clk: usize) -> Result<Self, Self::Err>
    where
        Self: Sized;
    fn set_power(&mut self, state: bool) -> Result<(), Self::Err>;
    fn set_intensity(&mut self, d: usize, i: u8) -> Result<(), Self::Err>;
    fn clear_display(&mut self, d: usize) -> Result<(), Self::Err>;
    fn write_raw_byte(&mut self, d: usize, header: u8, data: u8) -> Result<(), Self::Err>;
    fn write_buff(&mut self, d: usize, data: &[[bool; 8]; 8]) -> Result<(), Self::Err> {
        for (y, a) in data.iter().enumerate() {
            let mut v = 0;
            for (x, _) in a.iter().enumerate().filter(|(_, b)| **b) {
                v |= 1 << x;
            }
            self.write_raw_byte(d, y as u8 + 1, v)?;
        }
        Ok(())
    }
}
