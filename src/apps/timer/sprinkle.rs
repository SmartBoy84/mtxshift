use std::sync::LazyLock;

use rand::RngExt;

use crate::apps::timer::TimerType;

pub const SPRINKLE_FRAMES: TimerType = TimerType {
    frames: LazyLock::new(|| gen_sprinkle_frames()),
    preview: 30,
};

pub fn gen_sprinkle_frames() -> Vec<[[bool; 8]; 8]> {
    let mut frames = vec![];
    let mut display = [[false; 8]; 8];
    let mut rng = rand::rng();

    for _ in 0..8 * 8 {
        let mut x;
        let mut y;
        loop {
            x = rng.random_range(0..8);
            y = rng.random_range(0..8);
            if !display[y][x] {
                break;
            }
        }
        display[y][x] = true;
        frames.push(display.clone());
    }

    frames
}
