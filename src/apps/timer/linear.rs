use std::sync::LazyLock;

use crate::apps::timer::TimerType;

pub const LINEAR_FRAMES: TimerType = TimerType {
    frames: LazyLock::new(|| gen_linear_frames()),
    preview: 30,
};

// thank you gpt - icb
pub fn gen_linear_frames() -> Vec<[[bool; 8]; 8]> {
    let mut frames = vec![];
    let mut display = [[false; 8]; 8];
    let mut left_to_right = true;

    for row in 0..8 {
        for col in 0..8 {
            let index = if left_to_right { col } else { 7 - col };
            display[row][index] = true;
            frames.push(display.clone());
        }
        left_to_right = !left_to_right;
    }

    frames
}
