use std::{error::Error, fmt::Display};

use crate::hardware::{
    ButtonMonitor, ButtonMonitorFunctionality, DEBOUNCE_DUR, Matrix, MatrixFunctionality,
};

use async_compat::CompatExt;
use embedded_hal::digital::PinState;
use linux_embedded_hal::{
    CdevPin,
    gpio_cdev::{self, AsyncLineEventHandle, Chip, EventRequestFlags, LineRequestFlags},
};
use max7219::MAX7219;
use smol::{Timer, channel::TrySendError, future::FutureExt, stream::StreamExt};

const GPIO_NAME: &str = "/dev/gpiochip4"; // this is the path of gpiochip0 apparently...

fn get_output(chip: &mut Chip, off: u32) -> CdevPin {
    CdevPin::new(
        chip.get_line(off)
            .unwrap()
            .request(LineRequestFlags::OUTPUT, 0, "")
            .unwrap(),
    )
    .unwrap()
    .into_output_pin(PinState::Low)
    .unwrap()
}

impl MatrixFunctionality
    for Matrix<MAX7219<max7219::connectors::PinConnector<CdevPin, CdevPin, CdevPin>>>
{
    type Err = max7219::DataError;
    fn new(din: usize, cs: usize, clk: usize) -> Result<Self, Self::Err> {
        let mut gpio_chip = Chip::new(GPIO_NAME).unwrap();

        let din = get_output(&mut gpio_chip, 2);
        let cs = get_output(&mut gpio_chip, 3);
        let clk = get_output(&mut gpio_chip, 4);

        let mut display = MAX7219::from_pins(1, din, cs, clk)?;

        display.power_on()?;

        Ok(Matrix(display))
    }
    fn clear_display(&mut self, d: usize) -> Result<(), Self::Err> {
        self.0.clear_display(d)
    }
    fn set_intensity(&mut self, d: usize, i: u8) -> Result<(), Self::Err> {
        self.0.set_intensity(d, i)
    }
    fn set_power(&mut self, state: bool) -> Result<(), Self::Err> {
        match state {
            true => self.0.power_on(),
            false => self.0.power_off(),
        }
    }
    fn write_raw_byte(&mut self, d: usize, header: u8, data: u8) -> Result<(), Self::Err> {
        self.0.write_raw_byte(d, header, data)
    }
}

impl ButtonMonitorFunctionality for ButtonMonitor {
    fn monitor(&self) -> impl Future {
        async {
            println!("line: {}", self.line);
            let mut chip = Chip::new(GPIO_NAME).unwrap();

            let line = chip.get_line(self.line).unwrap();
            let mut events = AsyncLineEventHandle::new(
                line.events(
                    LineRequestFlags::INPUT,
                    EventRequestFlags::FALLING_EDGE,
                    format!("{}", self.line).as_str(),
                )
                .unwrap(),
            )
            .unwrap();

            loop {
                // button state changed
                let Some(_) = events.next().await else {
                    continue; // if error, then continue
                };

                // if any event (error or not occurs, restart timer)
                if {
                    async {
                        events.next().await;
                        true
                    }
                }
                .or(async {
                    Timer::after(DEBOUNCE_DUR).await;
                    false
                })
                .await
                {
                    // timer didn't complete
                    continue;
                }
                println!("BUTTON!");
                match self.tx.try_send(()) {
                    Err(TrySendError::Full(_)) => (),
                    e @ _ => e.unwrap(),
                };
            }
        }
        .compat()
    }
}
