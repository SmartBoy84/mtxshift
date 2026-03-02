use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Days, Local, NaiveTime, Timelike};

use smol::{
    Executor, Timer,
    channel::{self, Receiver, TrySendError},
    future::FutureExt,
    lock::Mutex,
};
use workjam_rs::{
    ApiClient, ApiRequest, ApiRequestWithPara, WorkjamUser,
    config::{HasCompanyID, HasEmployeeID, WorkjamRequestConfig},
    endpoints::{ApprovalReqs, Events},
    parameters::{ApprovalReqCatagory, ApprovalReqsPara, EventsPara},
    requests::{AuthRes, events},
};

use crate::hardware::{Matrix, MatrixFunctionality, SharedDisplay};

const HEARTBEAT: Duration = Duration::from_secs(1);

const SHIFT_CHECK_PERIOD: Duration = Duration::from_mins(5); // update every 5 min
const CHECK_DAYS: u64 = 7 * 3; // 3 weeks - left 3 columns

const TOKEN: &str = "eyJraWQiOiJXT1JLSkFNLUFQSS1HQVRFV0FZLUtFWS1JRCIsInR5cCI6IkpXVCIsImFsZyI6IlJTMjU2In0.eyJzdWIiOiIyMDAwMDQ2MDE5MzcwIiwiZmlyc3ROYW1lIjoiSGFtZGFuIiwibGFzdE5hbWUiOiJNYWhtb29kIiwiaXNXSkFkbWluIjpmYWxzZSwiZW50cm9weSI6NzYwNTIzMzk5NDc4NTUyNTI3NiwiaXNzIjoidW5rbm93biIsImRhdGFjZW50ZXJJZCI6ImdjcC1hdXMiLCJleHAiOjE3NzYwNTE0MTMsImlhdCI6MTc3MDg2NzM1MywiaXNYVG9rZW4iOnRydWV9.18NpP9QYLW829PPBTzOZXTMTwK9PpJEo9BuIsM2rdBxSghfWT_3grfhMC7xRr0jDsrnMkrqFV4xJzUHbBgnNPt2SRuvQMjt9O8II6xtGWqwCVbg7C8DggJrrXBt19PQevjpobb2pU6AJu-C8p72-3zFSE8detbki_E87btsWv_cq5c6CkTgOXy6k9E9NoMojc3lNiZ09jUC5i7Uzf_Gg7hcdU5bEiPyMQt9io0d0Mq6Sc5HRgTOmmMwhLlVd1b4zlJfpCYMlSC7-FbjXJsvMgvQZO-wTolwnpAN-40iZkRvT5Baw_qupYll_G39oDvsZ5vAqCy_Kh0ukg75OxIvYBA";

fn next_n_days(n: u64, now: DateTime<Local>) -> EventsPara {
    let now = now.with_time(NaiveTime::MIN).unwrap();
    EventsPara::builder()
        .start_date_time(now)
        .end_date_time(now.checked_add_days(Days::new(n)).unwrap())
        .build()
}

async fn loop_thing<T, D>(
    c: &WorkjamUser,
    events_req: &mut ApiRequestWithPara<Events>,
    display: &mut SharedDisplay<D>,
    my_config: &T,
    approval_req_req: &ApiRequestWithPara<ApprovalReqs>,
    offset: u64,
) -> anyhow::Result<()>
where
    T: HasCompanyID + HasEmployeeID,
    Matrix<D>: MatrixFunctionality,
{
    let now = Local::now().checked_add_days(Days::new(offset)).unwrap();
    display.lock().await.clear_display(0).unwrap();

    println!("Finding shifts");
    events_req.change_para(next_n_days(CHECK_DAYS, now));

    let events = c.request(events_req)?;

    let shifts = events
        .iter()
        .filter_map(|e| match e {
            events::Event::Shift(s) => Some(s),
            _ => None,
        })
        .collect::<Vec<_>>();
    println!("{} shifts in next {} days", shifts.len(), CHECK_DAYS);

    let mut todays_shift = None;

    for wk in 0..(CHECK_DAYS / 7) + 1 {
        let mut row = 0u8;
        for day in 0..6 {
            let day_start = now
                .with_time(NaiveTime::MIN)
                .unwrap()
                .checked_add_days(Days::new((wk as u64 * 7) + day as u64))
                .unwrap();

            let day_end = day_start.checked_add_days(Days::new(1)).unwrap();

            let Some(shift) = shifts.iter().find(|s| {
                s.start_date_time >= day_start
                    && s.end_date_time < day_end
                    && s.end_date_time >= day_start
                    && s.end_date_time < day_end
            }) else {
                continue;
            };

            if wk == 0 && day == 0 {
                // today's shift
                todays_shift = Some(shift);
            }

            println!("shift on day {}", wk as u64 * 7 + day as u64);
            row |= 1 << (7 - day);
        }
        display
            .lock()
            .await
            .write_raw_byte(0, (8 - wk) as u8, row)
            .unwrap()
    }

    if let Some(todays_shift) = todays_shift {
        let mut display = display.lock().await;
        display
            .write_raw_byte(
                0,
                3,
                (todays_shift.start_date_time.with_timezone(&Local).hour() as u8).reverse_bits(),
            )
            .unwrap();
        display
            .write_raw_byte(
                0,
                2,
                (todays_shift.start_date_time.with_timezone(&Local).minute() as u8).reverse_bits(),
            )
            .unwrap();

        println!("{}", todays_shift.id);
        let todays_shift_details = c.request(&todays_shift.details(my_config))?.segments;
        let mut segment_indicator = 0u8;

        let mut earliest_time = None;
        let mut earliest_position = None;

        for segment in todays_shift_details {
            if let Some(earliest_time) = earliest_time
                && earliest_time < segment.start_date_time
            {
                continue;
            }
            let segment_id = match segment.position.id.as_str() {
                "22779501" => {
                    println!("doin nightfill");
                    0
                } // nightfill
                "22779507" => {
                    println!("doin grocery");
                    1
                } // grocery
                "22779484" => {
                    println!("doin fresh con");
                    2
                } // fresh con
                _ => {
                    println!("UNKNOWN ROLE!!");
                    3
                } // unknown
            };

            earliest_time = Some(segment.start_date_time);
            earliest_position = Some(segment_id);

            segment_indicator |= 0b1000 << segment_id; // every shift is displayed after
        }
        segment_indicator |= 1 << earliest_position.unwrap(); // show first position in the first 3 leds

        display
            .write_raw_byte(0, 4, segment_indicator.reverse_bits())
            .unwrap();
        println!("shift today!");
    }

    // 6th column is pending approval requests
    let reqs = c.request(approval_req_req)?;
    println!("{} reqs pending!", reqs.len());
    display
        .lock()
        .await
        .write_raw_byte(
            0,
            5,
            if reqs.len() > u8::MAX as usize {
                u8::MAX
            } else {
                reqs.len() as u8
            },
        )
        .unwrap();

    Ok(())
}

pub async fn shift<D: 'static>(
    mut display: SharedDisplay<D>,
    ex: Arc<Executor<'_>>,
    rx: Receiver<()>,
    button: Receiver<()>,
) where
    Matrix<D>: MatrixFunctionality + Send,
{
    let indicator = Arc::new(Mutex::new(0u8));

    let heartbeat_display = display.clone();
    let heartbeat_indicator = indicator.clone();

    let _x = ex.spawn(async move {
        loop {
            Timer::after(HEARTBEAT).await;
            let mut indicator = heartbeat_indicator.lock().await;
            let mut display = heartbeat_display.lock().await;
            *indicator ^= 1;
            display.write_raw_byte(0, 1, *indicator).unwrap();
        }
    });

    let offset_indicator = indicator.clone();

    let day_offset = Arc::new(Mutex::new(0));
    let day_offset_clone = day_offset.clone();
    let (off_tx, off_rx) = channel::bounded::<()>(1);

    let button_monitor = ex.spawn(async move {
        loop {
            let Ok(_) = button.recv().await else {
                continue;
            };
            let mut day_offset = day_offset_clone.lock().await;
            let mut indicator = offset_indicator.lock().await;
            if *day_offset == 7 {
                *indicator &= !0x08; // set random ass middle led on to indicate that not on present day
                *day_offset = 0;
            } else {
                *indicator |= 0x08; // set random ass middle led on to indicate that not on present day
                *day_offset += 1;
            }
            match off_tx.try_send(()) {
                Err(TrySendError::Full(_)) => continue,
                r => r.unwrap(),
            }
        }
    });

    let c = WorkjamUser::new(TOKEN);
    let AuthRes { employers, user_id } = c.get_auth().unwrap();
    let my_config = WorkjamRequestConfig::new()
        .company_id(employers.into_iter().next().unwrap())
        .employee_id(user_id.to_string());

    let mut events_req =
        ApiRequest::<Events>::new(&my_config).add_para(&next_n_days(CHECK_DAYS, Local::now()));

    let approval_req_req = ApiRequest::<ApprovalReqs>::new(&my_config).add_para(
        &ApprovalReqsPara::builder()
            .category(ApprovalReqCatagory::MyRequests)
            .build(),
    );

    loop {
        let mut loop_indicator = indicator.lock().await;

        match loop_thing(
            &c,
            &mut events_req,
            &mut display,
            &my_config,
            &approval_req_req,
            *day_offset.lock().await,
        )
        .await
        {
            Ok(_) => *loop_indicator &= 0x7F, // think hex - turn off error indicator bit
            Err(e) => {
                println!("{e:?}");
                *loop_indicator |= 0x80; // flip on significant bit - HEX: split into 2 4 bit parts, highest of second part is 8 (2^3)
            }
        }

        display
            .lock()
            .await
            .write_raw_byte(0, 1, *loop_indicator)
            .unwrap();

        drop(loop_indicator);

        async {
            off_rx.recv().await.unwrap();
            println!("offset")
        }
        .or(async {
            // timeout, or page refresh should reset offset
            async {
                Timer::after(SHIFT_CHECK_PERIOD).await;
                println!("timeout")
            }
            .or(async {
                // on screen change, clear screen
                rx.recv().await.unwrap();
                display.lock().await.clear_display(0).unwrap();
                display.lock().await.set_power(true).unwrap();
                println!("page refresh")
            })
            .await;
            *day_offset.lock().await = 0;
            *indicator.lock().await &= !0x08; // flip off bit
        })
        .await;
    }
}
