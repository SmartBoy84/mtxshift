use std::sync::LazyLock;

use rand::RngExt;

use crate::apps::timer::TimerType;

use super::Coord;

pub const SAND_FRAMES_UNIFORM: TimerType = TimerType {
    frames: LazyLock::new(|| gen_sand_frames(true)),
    preview: 200,
};

pub const SAND_FRAMES_NON_UNIFORM: TimerType = TimerType {
    frames: LazyLock::new(|| gen_sand_frames(false)),
    preview: 200,
};

pub fn gen_sand_frames(uniform: bool) -> Vec<[[bool; 8]; 8]> {
    let spawn_x = 3;
    let mut current;

    let mut display = [[false; 8]; 9];
    let mut frames = vec![];

    let mut rnd = rand::rng();

    for i in 0..(display.len() - 1) * display[0].len() {
        current = Coord::new(
            if uniform {
                spawn_x
            } else {
                rnd.random_range(0..8)
            },
            0,
        );

        loop {
            // 64 available pixels
            display[*current.y()][*current.x()] = true;

            let mut b: [[bool; 8]; 8] = [[false; 8]; 8];
            b.clone_from_slice(&display[1..9]);

            frames.push(b);

            if *current.y() == display.len() - 1 {
                break;
            }

            let mut old = current.clone();
            // very inefficient but whatever
            match (
                (*current.x() > 0).then(|| display[*current.y() + 1][*current.x() - 1]), // down left
                display[*current.y() + 1][*current.x()],                                 // down
                (*current.x() < display[0].len() - 1)
                    .then(|| display[*current.y() + 1][*current.x() + 1]),
            ) {
                (_, false, _) => *current.y() += 1,
                (Some(false), _, Some(false)) => {
                    if rnd.random::<bool>() {
                        *current.x() += 1
                    } else {
                        *current.x() -= 1
                    }
                }
                (None | Some(true), _, Some(false)) => *current.x() += 1,
                (Some(false), _, None | Some(true)) => *current.x() -= 1,
                _ if *current.y() == 0 && i / 8 != 8 => {
                    match (
                        (*current.x() > 0).then(|| display[*current.y()][*current.x() - 1]), // left
                        (*current.x() < display[0].len() - 1)
                            .then(|| display[*current.y()][*current.x() + 1]),
                    ) {
                        (Some(false), Some(false)) => {
                            if rnd.random::<bool>() {
                                *current.x() += 1
                            } else {
                                *current.x() -= 1
                            }
                        }
                        (Some(false), _) => *current.x() -= 1,
                        (_, Some(false)) => *current.x() += 1,
                        _ => break, // about time!
                    }
                }
                _ => break,
            };
            display[*old.y()][*old.x()] = false;
        }
    }
    frames
}
