use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, routing::post, Json, Router};
use itertools::Itertools;
use model::{Coordinate, Space, Spaces, TurnRequest, TurnResponse, BOARD_SIZE};
use rand::prelude::*;

fn distance(from: Coordinate, to: Coordinate, spaces: Spaces) -> (usize, Vec<Coordinate>) {
    let mut visited = [[false; 20]; 20];
    let mut queue = VecDeque::new();
    let mut predecessors = HashMap::new();
    queue.push_back((from, 0));
    visited[from.x][from.y] = true;

    while let Some((current, dist)) = queue.pop_front() {
        if current == to {
            let mut path = vec![to];
            let mut step = current;
            while let Some(&prev) = predecessors.get(&step) {
                path.push(prev);
                step = prev;
            }
            path.reverse();
            return (dist, path);
        }

        for neighbor in current.surrounding() {
            if !visited[neighbor.x][neighbor.y] && spaces[neighbor.x][neighbor.y] != Space::Mountain
            {
                queue.push_back((neighbor, dist + 1));
                visited[neighbor.x][neighbor.y] = true;
                predecessors.insert(neighbor, current);
            }
        }
    }

    panic!("Should always be a path between two points");
}

async fn turn_handler(
    State(state): State<DistanceCache>,
    Json(body): Json<TurnRequest>,
) -> Json<Option<TurnResponse>> {
    let mut cache = state.lock().unwrap();
    let mut my_spaces_with_units = vec![];
    for x in 0..BOARD_SIZE {
        for y in 0..BOARD_SIZE {
            if body.spaces[x][y].owner() == Some(body.player) && body.spaces[x][y].get_units() > 0 {
                my_spaces_with_units.push(Coordinate { x, y });
            }
        }
    }
    my_spaces_with_units.shuffle(&mut thread_rng());

    let mut border_spaces = my_spaces_with_units
        .iter()
        .flat_map(|from| {
            from.surrounding().into_iter().filter(|to| {
                body.spaces[to.x][to.y] != Space::Mountain
                    && body.spaces[to.x][to.y].owner() != Some(body.player)
            })
        })
        .unique()
        .collect_vec();
    border_spaces.shuffle(&mut thread_rng());

    let mut least_moves = (
        usize::MAX,
        Coordinate { x: 0, y: 0 },
        Coordinate { x: 0, y: 0 },
    );
    for my_space in my_spaces_with_units {
        for their_space in &border_spaces {
            let my_units = body.spaces[my_space.x][my_space.y].get_units();
            let their_units = body.spaces[their_space.x][their_space.y].get_units();
            if my_units > their_units {
                let (distance, path) = cache
                    .entry((my_space, *their_space))
                    .or_insert_with(|| distance(my_space, *their_space, body.spaces));
                if *distance < least_moves.0 {
                    least_moves = (*distance, my_space, path[1]);
                }
            }
        }
    }

    if least_moves.0 != usize::MAX {
        Json(Some(TurnResponse {
            from: least_moves.1,
            to: least_moves.2,
        }))
    } else {
        Json(None)
    }
}

type DistanceCache = Arc<Mutex<HashMap<(Coordinate, Coordinate), (usize, Vec<Coordinate>)>>>;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .expect("Should pass one argument, the port to run on")
        .parse()
        .expect("First argument should be a valid port");

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    let cache: DistanceCache = Arc::new(Mutex::new(HashMap::new()));

    axum::serve(
        listener,
        Router::new()
            .route("/", post(turn_handler))
            .with_state(cache),
    )
    .await
    .unwrap();
}
