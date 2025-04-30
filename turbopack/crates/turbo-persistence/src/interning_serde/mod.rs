//! Exposed for usage in `turbo-tasks-backend`

use std::{
    io::{Read, Write},
    sync::LazyLock,
};

use anyhow::Context;
use dashmap::DashMap;
use serde::{de::DeserializeOwned, Serialize};
use turbo_rcstr::RcStr;

static GLOBAL_INTERN_MAP: LazyLock<DashMap<RcStr, u32>> = LazyLock::new(DashMap::new);
static GLOBAL_INTERN_MAP_REVERSE: LazyLock<DashMap<u32, RcStr>> = LazyLock::new(DashMap::new);

pub fn to_vec<T>(
    config: &pot::Config,
    value: &T,
    get_global_id: impl FnMut(&RcStr) -> anyhow::Result<u32>,
) -> anyhow::Result<Vec<u8>>
where
    T: Serialize,
{
    let mut vec = Vec::new();
    to_writer(config, value, &mut vec, get_global_id)?;
    Ok(vec)
}

#[inline(never)] // Mutex outside of the hot path
fn store_in_memory_cache(s: &RcStr, global_id: u32) -> u32 {
    GLOBAL_INTERN_MAP_REVERSE.insert(global_id, s.clone());
    *GLOBAL_INTERN_MAP
        .entry(s.clone())
        .or_insert_with(|| global_id)
}

pub fn to_writer<T, W>(
    config: &pot::Config,
    value: &T,
    mut writer: W,
    mut get_global_id: impl FnMut(&RcStr) -> anyhow::Result<u32>,
) -> anyhow::Result<()>
where
    T: Serialize,
    W: Write,
{
    let (result, ser_map) = turbo_rcstr::set_ser_map(|| config.serialize(value));
    let value = result?;

    let mut intern_map = Vec::with_capacity(ser_map.len());

    for s in ser_map.iter() {
        if let Some(id) = GLOBAL_INTERN_MAP.get(s).as_deref().copied() {
            intern_map.push(id);
        } else {
            let id = get_global_id(s).context("failed to get global id")?;
            store_in_memory_cache(s, id);
            intern_map.push(id);
        }
    }

    writer.write_all(&intern_map.len().to_le_bytes())?;
    for &id in &intern_map {
        writer.write_all(&id.to_le_bytes())?;
    }

    writer.write_all(&value)?;

    Ok(())
}

#[inline(never)] // Mutex outside of the hot path
fn restore_strings_with_in_memory_cache(
    intern_map: Vec<u32>,
    mut query_db: impl FnMut(u32) -> anyhow::Result<RcStr>,
) -> anyhow::Result<Vec<RcStr>> {
    let missing = intern_map
        .iter()
        .copied()
        .filter(|global_id| GLOBAL_INTERN_MAP_REVERSE.get(global_id).is_none())
        .collect::<Vec<_>>();

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
