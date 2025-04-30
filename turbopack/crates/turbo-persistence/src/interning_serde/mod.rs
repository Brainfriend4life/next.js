//! Exposed for usage in `turbo-tasks-backend`

use std::{
    io::{Read, Write},
    sync::LazyLock,
};

use dashmap::DashMap;
use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;
use serde::{de::DeserializeOwned, Serialize};
use turbo_rcstr::RcStr;

static GLOBAL_INTERN_MAP: LazyLock<DashMap<RcStr, u32>> = LazyLock::new(DashMap::new);
static GLOBAL_INTERN_MAP_REVERSE: LazyLock<DashMap<u32, RcStr>> = LazyLock::new(DashMap::new);

pub fn to_vec<T>(config: &pot::Config, value: &T) -> anyhow::Result<(Vec<u8>, RcStrToLocalId)>
where
    T: Serialize,
{
    let mut vec = Vec::new();
    let ser_map = to_writer(config, value, &mut vec)?;
    Ok((vec, ser_map))
}

#[inline(never)] // Mutex outside of the hot path
fn store_in_memory_cache(s: &RcStr, global_id: u32) -> u32 {
    GLOBAL_INTERN_MAP_REVERSE.insert(global_id, s.clone());
    *GLOBAL_INTERN_MAP
        .entry(s.clone())
        .or_insert_with(|| global_id)
}

#[derive(Default)]
pub struct RcStrToLocalId(IndexSet<RcStr, FxBuildHasher>);

pub fn to_writer<T, W>(config: &pot::Config, value: &T, writer: W) -> anyhow::Result<RcStrToLocalId>
where
    T: Serialize,
    W: Write,
{
    let (result, ser_map) = turbo_rcstr::set_ser_map(|| config.serialize_into(value, writer));
    result?;

    Ok(RcStrToLocalId(ser_map))
}

#[inline(never)] // Mutex outside of the hot path
fn restore_strings_with_in_memory_cache(
    intern_map: Vec<u32>,
    mut query_db: impl FnMut(u32) -> anyhow::Result<RcStr>,
) -> anyhow::Result<Vec<RcStr>> {
    let missing = intern_map
        .iter()
        .copied()
        .filter(|global_id| GLOBAL_INTERN_MAP_REVERSE.get(global_id).is_none());

    for global_id in missing {
        let s = query_db(global_id)?;
        store_in_memory_cache(&s, global_id);
    }

    let mut result = Vec::with_capacity(intern_map.len());
    for id in intern_map {
        result.push(GLOBAL_INTERN_MAP_REVERSE.get(&id).unwrap().clone());
    }
    Ok(result)
}

pub fn from_slice<T>(
    config: &pot::Config,
    slice: &[u8],
    query_db: impl FnMut(u32) -> anyhow::Result<RcStr>,
) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let mut reader = std::io::Cursor::new(slice);

    let mut intern_map = Vec::new();

    let mut len = [0; 4];
    reader.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);

    for _ in 0..len {
        let mut id = [0; 4];
        reader.read_exact(&mut id)?;
        intern_map.push(u32::from_le_bytes(id));
    }

    let de_map = restore_strings_with_in_memory_cache(intern_map, query_db)?;

    turbo_rcstr::set_de_map(&de_map, || Ok(config.deserialize_from(&mut reader)?))
}
