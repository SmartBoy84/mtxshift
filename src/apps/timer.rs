use std::{
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use smol::{Executor, Timer, channel::Receiver, future::FutureExt};

use crate::{
    apps::timer::{
        linear::LINEAR_FRAMES,
        sand::{SAND_FRAMES_NON_UNIFORM, SAND_FRAMES_UNIFORM},
        sprinkle::SPRINKLE_FRAMES,
    },
    hardware::{Matrix, MatrixFunctionality, SharedDisplay},
};

mod linear;
mod sand;
mod sprinkle;

const FLASH_PERIOD: Duration = Duration::from_secs(1);
const SELECT_PERIOD: Duration = Duration::from_millis(250); // 8 leds total -> 2 seconds to confirm

#[derive(Debug, Clone)]
struct Coord(usize, usize);
impl Coord {
    fn new(start_x: usize, start_y: usize) -> Self {
        Self(start_x, start_y)
    }
    fn x(&mut self) -> &mut usize {
        &mut self.0
    }
    fn y(&mut self) -> &mut usize {
        &mut self.1
    }
}

struct TimerType {
    preview: usize,
    frames: LazyLock<Vec<[[bool; 8]; 8]>>,
}

const TIMERS: [TimerType; 4] = [
    SAND_FRAMES_UNIFORM,
    SAND_FRAMES_NON_UNIFORM,
    SPRINKLE_FRAMES,
    LINEAR_FRAMES,
];

async fn select<D>(t: Duration, display: SharedDisplay<D>, button: Receiver<()>) -> bool
where
    Matrix<D>: MatrixFunctionality,
{
    // first completely show preview, then countdown
    async {
        let mut v: u8 = 0;
        display.lock().await.write_raw_byte(0, 8, v).unwrap();
        for i in 0..=7 {
            Timer::after(t).await;
            v |= 1 << i;
            display.lock().await.write_raw_byte(0, 8, v).unwrap();
        }
        true
    }
    .or(async {
        // or user pressed button - go to next
        button.recv().await.unwrap();
        false
    })
    .await
}

pub async fn app<D>(display: SharedDisplay<D>, button: Receiver<()>)
where
    Matrix<D>: MatrixFunctionality,
{
    let timers = TIMERS;
    let mut timers_iter = timers.iter().cycle();

    let timer = loop {
        let timer = timers_iter.next().unwrap();

        // render preview
        display
            .lock()
            .await
            .write_buff(0, &timer.frames[timer.preview])
            .unwrap();

        // wait for selection
        if select(SELECT_PERIOD, display.clone(), button.clone()).await {
            break timer;
        }
    };
    display.lock().await.clear_display(0).unwrap();

    // next step - select the timer interval!
    let mut hour = 0;
    loop {
        display
            .lock()
            .await
            .write_raw_byte(0, 2, if hour == 0 { 0 } else { 1 << (hour - 1) })
            .unwrap();
        if select(SELECT_PERIOD, display.clone(), button.clone()).await {
            break;
        }
        hour = if hour == 8 { 0 } else { hour + 1 };
    }

    let mut minute = 6; // default is 45 minutes
    loop {
        display
            .lock()
            .await
            .write_raw_byte(0, 1, if minute == 0 { 0 } else { 1 << (minute - 1) })
            .unwrap();
        if select(SELECT_PERIOD, display.clone(), button.clone()).await {
            break;
        }
        minute = if minute == 8 { 0 } else { minute + 2 }; // i.e., go up by 15 mins (2/8 of an hour)
    }
    let minute = minute as f32 * 60. / 8.;

    let duration = Duration::from_secs((minute.fract() * 60.) as u64)
        + Duration::from_mins(minute.trunc() as u64)
        + Duration::from_hours(hour);

    println!("{duration:?}");

    // final step - start timer!
    let interval = duration / timer.frames.len() as u32;
    let mut elapsed = Duration::from_secs(0);
    for (i, frame) in timer.frames.iter().enumerate() {
        let segment_start = Instant::now();

        let target_time = interval * i as u32;
        let pause = async {
            Timer::after(target_time.saturating_sub(elapsed)).await;

            display.lock().await.write_buff(0, frame).unwrap();
            false
        }
        .or(async {
            button.recv().await.unwrap();
            true
        })
        .await;
        elapsed += segment_start.elapsed();
        if pause {
            display.lock().await.set_power(false).unwrap();
            Timer::after(Duration::from_millis(150)).await;
            display.lock().await.set_power(true).unwrap();

            button.recv().await.unwrap(); // wait for another button press to resume

            display.lock().await.set_power(false).unwrap();
            Timer::after(Duration::from_millis(150)).await;
            display.lock().await.set_power(true).unwrap();
        }
    }
}

pub async fn blinky<D: 'static>(
    display: SharedDisplay<D>,
    _ex: Arc<Executor<'_>>,
    rx: Receiver<()>,
    button: Receiver<()>,
) where
    Matrix<D>: MatrixFunctionality + Send,
{
    let app_display = display.clone();
    loop {
        // app restarted, reset display (no background timer - spoils the purpose!)
        display.lock().await.clear_display(0).unwrap();
        display.lock().await.set_power(true).unwrap();

        async {
            app(app_display.clone(), button.clone()).await;

            // once it finishes normally, flash to indicate until screen changed (i.e., other future resolves)
            for i in 0..=7 {
                display.lock().await.write_raw_byte(0, i, 0xFF).unwrap();
            }

            for s in [false, true].iter().cycle() {
                display.lock().await.set_power(*s).unwrap();
                Timer::after(FLASH_PERIOD).await;
            }
        }
        .or(async {
            rx.recv().await.unwrap();
        })
        .or(async {
            button.recv().await.unwrap();
        })
        .await;
    }
}
