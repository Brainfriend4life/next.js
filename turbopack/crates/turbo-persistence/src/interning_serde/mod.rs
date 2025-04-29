//! Exposed for usage in `turbo-tasks-backend`

use std::{
    io::{Read, Write},
    sync::LazyLock,
};

use dashmap::DashMap;
use serde::{de::DeserializeOwned, Serialize};
use turbo_rcstr::RcStr;

static GLOBAL_INTERN_MAP: LazyLock<DashMap<RcStr, u32>> = LazyLock::new(DashMap::new);
static GLOBAL_INTERN_MAP_REVERSE: LazyLock<DashMap<u32, RcStr>> = LazyLock::new(DashMap::new);

pub fn to_vec<T>(config: &pot::Config, value: &T) -> pot::Result<Vec<u8>>
where
    T: Serialize,
{
    let mut vec = Vec::new();
    to_writer(config, value, &mut vec)?;
    Ok(vec)
}

#[inline(never)] // Mutex outside of the hot path
fn intern_str(s: &RcStr) -> u32 {
    *GLOBAL_INTERN_MAP.entry(s.clone()).or_insert_with(|| {
        let id = GLOBAL_INTERN_MAP.len() as u32;
        GLOBAL_INTERN_MAP_REVERSE.insert(id, s.clone());
        id
    })
}

#[inline(never)] // Mutex outside of the hot path
fn restore_str(id: u32) -> RcStr {
    GLOBAL_INTERN_MAP_REVERSE.get(&id).unwrap().clone()
}

pub fn to_writer<T, W>(config: &pot::Config, value: &T, mut writer: W) -> pot::Result<()>
where
    T: Serialize,
    W: Write,
{
    let (result, ser_map) = turbo_rcstr::set_ser_map(|| config.serialize(value));
    let value = result?;

    let mut intern_map = Vec::with_capacity(ser_map.len());

    for s in ser_map {
        intern_map.push(intern_str(&s));
    }

    writer.write_all(&intern_map.len().to_le_bytes())?;
    for &id in &intern_map {
        writer.write_all(&id.to_le_bytes())?;
    }

    writer.write_all(&value)?;

    Ok(())
}

pub fn from_slice<T>(config: &pot::Config, slice: &[u8]) -> pot::Result<T>
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

    let mut de_map = Vec::with_capacity(intern_map.len());

    for &id in &intern_map {
        de_map.push(restore_str(id));
    }

    turbo_rcstr::set_de_map(&de_map, || config.deserialize_from(&mut reader))
}
