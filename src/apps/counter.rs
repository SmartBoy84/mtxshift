use std::{io::SeekFrom, sync::Arc, time::Duration};

use smol::{
    Executor, Timer, channel::{Receiver, Sender}, fs, future, io::{AsyncSeekExt, AsyncWriteExt}, lock::Mutex
};

use crate::{
    Intensity,
    hardware::{Matrix, MatrixFunctionality, SharedDisplay},
};

use futures::{self, AsyncReadExt, select, FutureExt};

const COUNT_PATH: &str = "/home/hamdan/special_count.txt";

pub async fn counter<D: 'static>(
    display: SharedDisplay<D>,
    ex: Arc<Executor<'_>>,
    rx: Receiver<()>,
    button: Receiver<()>,
    _: Sender<Intensity>,
) where
    Matrix<D>: MatrixFunctionality + Send,
{
    let disp_cache = Arc::new(Mutex::new([0u8; 7])); // last line reserved

    let (display_tx, display_rx) = smol::channel::unbounded::<()>();
    let (update_tx, update_rx) = smol::channel::unbounded::<()>();
    let (reset_tx, reset_rx) = smol::channel::unbounded::<()>();

{
    let disp_cache = disp_cache.clone();
    ex.spawn(async move {
        let mut f = fs::OpenOptions::new().read(true).write(true).create(true).open(COUNT_PATH).await.unwrap();
        
        let mut n = {
            let mut b = [0u8; size_of::<usize>()];
            
            match f.read_exact(&mut b).await {
                Ok(_) => usize::from_le_bytes(b),
                Err(_) => 0
            }
        };

        loop {
            disp_cache.lock().await[n/8] = (1 << (n % 8))-1;
            display_tx.send(()).await.unwrap();
            println!("updated!");

            update_rx.recv().await.unwrap(); // increment!
            println!("increment request");
            n += 1;
            f.set_len(0).await.unwrap();
            f.seek(SeekFrom::Start(0)).await.unwrap();
            f.write(&n.to_le_bytes()).await.unwrap();
            f.flush().await.unwrap();
        }
    }).detach()}

        {
                let display = display.clone();
                let button = button.clone();

                ex.spawn(async move {
                    let mut n = 0;
                    const DEC_DUR: Duration = Duration::from_secs(1);

                    loop {
                        display.lock().await.write_raw_byte(0, 8, 0).unwrap();
                        
                        loop {
                            select! {
                                () = async {button.recv().await.unwrap()}.fuse() => n += 1,
                                () = async {if n > 0 {Timer::after(DEC_DUR).await;} else {future::pending::<()>().await;}}.fuse() => n -= 1,
                                _ = async {reset_rx.recv().fuse().await.unwrap()}.fuse() => {
                                    n = 0;
                                    break;
                                }
                            };

                            if n >= 9 { // go "off" to confirm
                                n = 0;
                                update_tx.send(()).await.unwrap();
                                break;
                            } else if n == 0 {
                                break;
                            }

                            const MASKS: [u8; 9] = [0x00, 0x01, 0x03, 0x07, 0x0F, 0x1F, 0x3F, 0x7F, 0xFF];
                            display.lock().await.write_raw_byte(0, 8, MASKS[n]).unwrap();
                        }
                    }
                }).detach();
            };

    loop {
        println!("starting counter");

        let mut disp = display.lock().await;
        println!("writing: {:?}", disp_cache.lock().await);
        for (i, v) in disp_cache.lock().await.iter().enumerate() {
            disp.write_raw_byte(0, i as u8 + 1, *v).unwrap();
        }
        drop(disp); // obv drop so other tasks can handle it
        
        select! {
            _ = async {display_rx.recv().await.unwrap();}.fuse() => (),
            _ = async {rx.recv().await.unwrap();}.fuse() => {
                reset_tx.send(()).await.unwrap();
                display.lock().await.write_raw_byte(0, 8, 0).unwrap();

            }
        };
    }
}
