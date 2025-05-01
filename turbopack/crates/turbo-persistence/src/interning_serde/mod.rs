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
pub struct RcStrToLocalId(pub IndexSet<RcStr, FxBuildHasher>);

#[derive(Default)]
pub struct LocalIdToGlobalId(Vec<u32>);

impl From<Vec<u32>> for LocalIdToGlobalId {
    fn from(value: Vec<u32>) -> Self {
        Self(value)
    }
}

impl LocalIdToGlobalId {
    pub fn write_to(&self, writer: &mut impl Write) -> anyhow::Result<()> {
        let len = self.0.len() as u32;
        writer.write_all(&len.to_le_bytes())?;
        for id in self.0.iter() {
            writer.write_all(&id.to_le_bytes())?;
        }
        Ok(())
    }

    fn read_from(reader: &mut impl Read) -> anyhow::Result<Self> {
        let mut global_ids = Vec::new();

        let mut len = [0; 4];
        reader.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len);
        global_ids.reserve(len as usize);

        for _ in 0..len {
            let mut id = [0; 4];
            reader.read_exact(&mut id)?;
            global_ids.push(u32::from_le_bytes(id));
        }

        Ok(Self(global_ids))
    }
}

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
    global_ids: Vec<u32>,
    mut query_db: impl FnMut(u32) -> anyhow::Result<RcStr>,
) -> anyhow::Result<Vec<RcStr>> {
    let missing = global_ids
        .iter()
        .copied()
        .filter(|global_id| GLOBAL_INTERN_MAP_REVERSE.get(global_id).is_none());

    for global_id in missing {
        let s = query_db(global_id)?;
        store_in_memory_cache(&s, global_id);
    }

    let mut result = Vec::with_capacity(global_ids.len());
    for id in global_ids {
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

    let global_ids = LocalIdToGlobalId::read_from(&mut reader)?;

    let de_map = restore_strings_with_in_memory_cache(global_ids.0, query_db)?;

    turbo_rcstr::set_de_map(&de_map, || Ok(config.deserialize_from(&mut reader)?))
}
