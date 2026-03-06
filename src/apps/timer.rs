use std::{
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use futures::TryFutureExt;
use smol::{
    Executor, Timer,
    channel::{Receiver, Sender, TrySendError},
    future::FutureExt as SmolFutureExt,
};

use crate::{
    Intensity,
    apps::{
        PauseType,
        timer::{
            linear::LINEAR_FRAMES,
            sand::{SAND_FRAMES_NON_UNIFORM, SAND_FRAMES_UNIFORM},
            sprinkle::SPRINKLE_FRAMES,
        },
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

/// WARNING; I declare that this app handles global_pause -> I must always handle it as the main loop waits for tx
pub async fn app<D>(
    display: SharedDisplay<D>,
    ex: Arc<Executor<'_>>,
    button: Receiver<()>,
    pause: Receiver<PauseType>,
    global_pause: Sender<Intensity>,
) where
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

    /*
    When we are in the timer screen, the pause button triggers a global pause
    -> this just unifies everything
    when the timer ends, this task is dropped so button no longer mapped
     */
    let button_to_global_task = ex.spawn(async move {
        button.recv().await.unwrap();
        match global_pause.try_send(Intensity::Pause(true)) {
            Err(TrySendError::Full(_)) => (),
            r => r.unwrap(),
        }
    });

    for (i, frame) in timer.frames.iter().enumerate() {
        let segment_start = Instant::now();

        let target_time = interval * i as u32;
        let pause_event = async {
            Timer::after(target_time.saturating_sub(elapsed)).await; // automatically account for drift + if timer wasn't able to complete due to pause

            display.lock().await.write_buff(0, frame).unwrap();
            None
        }
        .or(async {
            let k = pause.recv().map_ok(|t| Some(t)).await.unwrap();
            println!("got pause request!!");
            k
        })
        .await;
        elapsed += segment_start.elapsed(); // to account for when Timer stopped early (since it uses Instant under the hood)

        // this is safe - we pause at a time where we aren't measuring elapsed time -> no sporadic jumps
        if let Some(t) = pause_event {
            // pretty sick - I don't even need to destructure the internal Option - just need to keep it so that it isn't droppd
            // now the outer user waits for this to complete and then PauseTracker (if it exists) is dropped!
            display.lock().await.set_power(false).unwrap();
            Timer::after(Duration::from_millis(150)).await;
            display.lock().await.set_power(true).unwrap();

            drop(t);

            // wait for unpause to avoid timer going ahead
            while !matches!(pause.recv().await.unwrap(), PauseType::Unpause) {}

            println!("paused app!")
        }
    }
    // button -> global pause task dropped here
    drop(button_to_global_task)
}

pub async fn timer<D: 'static>(
    display: SharedDisplay<D>,
    ex: Arc<Executor<'_>>,
    rx: Receiver<()>,
    button: Receiver<()>,
    pause: Receiver<PauseType>,
    global_pause: Sender<Intensity>,
) where
    Matrix<D>: MatrixFunctionality + Send,
{
    let app_display = display.clone();

    loop {
        // app restarted, reset display (no background timer - spoils the purpose!)
        display.lock().await.clear_display(0).unwrap();
        display.lock().await.set_power(true).unwrap();

        let outer_pause = pause.clone();
        async {
            let (pause_tx, pause_rx) = smol::channel::bounded::<PauseType>(1);
            let pause = outer_pause.clone();
            let pause_relay_task = ex.spawn(async move {
                loop {
                    let _ = pause_tx.send(pause.clone().recv().await.unwrap());
                } // simple relay
            });

            app(
                app_display.clone(),
                ex.clone(),
                button.clone(),
                pause_rx,
                global_pause.clone(),
            )
            .await;

            drop(pause_relay_task); // to stop the relay listener
            let pause = outer_pause.clone();
            let _pause_chomper_task = ex.spawn(async move {
                loop {
                    let _ = pause.recv().await; // receive and drop
                }
            });

            // once it finishes normally, flash to indicate until screen changed (i.e., other future resolves)
            for i in 0..=7 {
                display.lock().await.write_raw_byte(0, i, 0xFF).unwrap();
            }

            async {
                for s in [false, true].iter().cycle() {
                    display.lock().await.set_power(*s).unwrap();
                    Timer::after(FLASH_PERIOD).await;
                }
            }
            .or(async {
                button.recv().await.unwrap(); // safe to handle button here, as the other task using it is done
            })
            .await;
        }
        .or(async {
            rx.recv().await.unwrap();
        })
        .await;
    }
}
