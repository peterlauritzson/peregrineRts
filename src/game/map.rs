use bevy::prelude::*;
use serde::{Serialize, Deserialize};
use crate::game::fixed_math::{FixedVec2, FixedNum};
use crate::game::pathfinding::HierarchicalGraph;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use flate2::write::ZlibEncoder;
use flate2::read::ZlibDecoder;
use flate2::Compression;

pub const MAP_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MapSize {
    pub top_left: FixedVec2,
    pub bottom_right: FixedVec2,
}

impl MapSize {
    pub fn get_width(&self) -> FixedNum {
        self.bottom_right.x - self.top_left.x
    }

    pub fn get_height(&self) -> FixedNum {
        self.bottom_right.y - self.top_left.y
    }
}

#[derive(Serialize, Deserialize)]
pub struct MapData {
    pub version: u32,
    pub size: MapSize,
    // pub map_width: FixedNum,
    // pub map_height: FixedNum,
    pub cell_size: FixedNum,
    pub cluster_size: usize,
    pub obstacles: Vec<MapObstacle>,
    pub start_locations: Vec<StartLocation>,
    pub cost_field: Vec<u8>,
    pub graph: HierarchicalGraph,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MapObstacle {
    pub position: FixedVec2,
    pub radius: FixedNum,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StartLocation {
    pub player_id: u8,
    pub position: FixedVec2,
}

pub fn save_map(path: &str, map_data: &MapData) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = ZlibEncoder::new(writer, Compression::default());
    bincode::serialize_into(&mut encoder, map_data)?;
    Ok(())
}

pub fn load_map(path: &str) -> Result<MapData, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut decoder = ZlibDecoder::new(reader);
    let map_data: MapData = bincode::deserialize_from(&mut decoder)?;
    Ok(map_data)
}
