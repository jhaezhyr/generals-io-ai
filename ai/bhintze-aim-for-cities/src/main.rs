use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    fmt::Display,
    net::SocketAddr,
};

use itertools::{self, Itertools};

use axum::{routing::post, Json, Router};
use model::{Coordinate, Space, TurnRequest, TurnResponse, BOARD_SIZE};
use strum_macros::Display;

trait CoordinateExtras {
    fn neighbors(&self, board_size: usize) -> Vec<Coordinate>;
    fn distance(&self, other: &Coordinate) -> i32;
}

#[repr(i32)]
#[derive(Display, PartialEq, Eq, PartialOrd, Ord)]
enum TargetPriority {
    Mountain = -1,
    MyTile = 0,
    EmptyTile = 1,
    EnemyTile = 2,
    EmptyTower = 3,
    EnemyTower = 4,
    EnemyCapital = 5,
}

impl CoordinateExtras for Coordinate {
    fn neighbors(&self, board_size: usize) -> Vec<Coordinate> {
        let mut result: Vec<Coordinate> = vec![];
        if self.y > 0 {
            result.push(Coordinate {
                x: self.x,
                y: self.y - 1,
            });
        }
        if self.y < board_size - 1 {
            result.push(Coordinate {
                x: self.x,
                y: self.y + 1,
            });
        }
        if self.x > 0 {
            result.push(Coordinate {
                x: self.x - 1,
                y: self.y,
            });
        }
        if self.x < board_size - 1 {
            result.push(Coordinate {
                x: self.x + 1,
                y: self.y,
            });
        }
        result
    }

    fn distance(&self, other: &Coordinate) -> i32 {
        ((self.x.try_into() as Result<i32, _>).unwrap()
            - (other.x.try_into() as Result<i32, _>).unwrap())
        .abs()
            + ((self.y.try_into() as Result<i32, _>).unwrap()
                - (other.y.try_into() as Result<i32, _>).unwrap())
            .abs()
    }
}

fn a_star<F>(
    start: Coordinate,
    goal: Coordinate,
    board_size: usize,
    is_passable: F,
) -> Option<Vec<Coordinate>>
where
    F: Fn(&Coordinate) -> bool,
{
    #[derive(Copy, Clone, Eq, PartialEq)]
    struct Node {
        coordinate: Coordinate,
        cost: i32,
    }

    impl Ord for Node {
        fn cmp(&self, other: &Node) -> Ordering {
            other.cost.cmp(&self.cost)
        }
    }

    impl PartialOrd for Node {
        fn partial_cmp(&self, other: &Node) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<Coordinate, Coordinate> = HashMap::new();
    let mut g_score: HashMap<Coordinate, i32> = HashMap::new();
    let mut f_score: HashMap<Coordinate, i32> = HashMap::new();

    open_set.push(Node {
        coordinate: start,
        cost: 0,
    });
    g_score.insert(start, 0);
    f_score.insert(start, start.distance(&goal));

    while let Some(Node {
        coordinate: current,
        ..
    }) = open_set.pop()
    {
        if current == goal {
            let mut path = vec![current];
            let mut current = current;
            while let Some(&parent) = came_from.get(&current) {
                path.push(parent);
                current = parent;
            }
            path.reverse();
            return Some(path);
        }

        for neighbor in current.neighbors(board_size) {
            if !is_passable(&neighbor) {
                continue;
            }

            let tentative_g_score = g_score.get(&current).unwrap_or(&i32::MAX) + 1;
            if tentative_g_score < *g_score.get(&neighbor).unwrap_or(&i32::MAX) {
                came_from.insert(neighbor, current);
                g_score.insert(neighbor, tentative_g_score);
                f_score.insert(neighbor, tentative_g_score + neighbor.distance(&goal));
                open_set.push(Node {
                    coordinate: neighbor,
                    cost: tentative_g_score + neighbor.distance(&goal),
                });
            }
        }
    }
    None
}

async fn turn_handler(Json(body): Json<TurnRequest>) -> Json<Option<TurnResponse>> {
    let priorities_and_strengths_of_targets: Vec<_> = (0..BOARD_SIZE)
        .flat_map(|x| (0..BOARD_SIZE).map(move |y| Coordinate { x, y }))
        .map(|c| match body.spaces[c.x][c.y] {
            Space::NeutralTown { units } => (c, TargetPriority::EmptyTower, units),
            Space::PlayerTown { owner, units } => {
                if owner == body.player {
                    (c, TargetPriority::MyTile, units)
                } else {
                    (c, TargetPriority::EnemyTower, units)
                }
            }

            Space::PlayerCapital { owner, units } => {
                if owner == body.player {
                    (c, TargetPriority::MyTile, units)
                } else {
                    (c, TargetPriority::EnemyCapital, units)
                }
            }

            Space::Mountain => (c, TargetPriority::Mountain, 10000),
            Space::Empty => (c, TargetPriority::EmptyTile, 0),
            Space::PlayerEmpty { owner, units } => {
                if owner == body.player {
                    (c, TargetPriority::MyTile, units)
                } else {
                    (c, TargetPriority::EnemyTile, units)
                }
            }
        })
        .sorted_by(|lh, rh| rh.1.cmp(&lh.1))
        .collect();
    if let Some((target_loc, target_priority, target_units)) =
        priorities_and_strengths_of_targets.first()
    {
        println!("targetspace={}, priority={}", target_loc, target_priority);
        let strengths_of_my_spaces: Vec<_> = (0..BOARD_SIZE)
            .flat_map(|x| (0..BOARD_SIZE).map(move |y| Coordinate { x, y }))
            .flat_map(|c| {
                if body.spaces[c.x][c.y].owner() == Some(body.player) {
                    Some((c, body.spaces[c.x][c.y].get_units()))
                } else {
                    None
                }
            })
            .collect();
        let closest_army_that_is_big_enough = strengths_of_my_spaces
            .iter()
            .filter(|(_, u)| u > target_units)
            .sorted_by(|lh, rh| lh.0.distance(&target_loc).cmp(&rh.0.distance(&target_loc)))
            .next();
        let biggest_army = strengths_of_my_spaces
            .iter()
            .sorted_by(|lh, rh| rh.1.cmp(&lh.1))
            .next();
        if let Some(loc_of_biggest_army) = closest_army_that_is_big_enough.or(biggest_army) {
            println!(
                "biggestarmy={}, size={}",
                loc_of_biggest_army.0, loc_of_biggest_army.1
            );
            if let Some(next_steps) = a_star(
                loc_of_biggest_army.0,
                *target_loc,
                BOARD_SIZE.try_into().unwrap(),
                |c: &Coordinate| -> bool {
                    match body.spaces[c.x][c.y] {
                        Space::Mountain => false,
                        _ => true,
                    }
                },
            ) {
                println!("path={}", next_steps.iter().join(" "));
                if let Some(next_step) = next_steps.get(1) {
                    println!(
                        "Take from {} and put it in {next_step}",
                        loc_of_biggest_army.0
                    );
                    return Json(Some(TurnResponse {
                        from: loc_of_biggest_army.0,
                        to: next_step.to_owned(),
                    }));
                }
            }
        }
    }
    return Json(None);
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .expect("Should pass one argument, the port to run on")
        .parse()
        .expect("First argument should be a valid port");

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, Router::new().route("/", post(turn_handler)))
        .await
        .unwrap();
}
