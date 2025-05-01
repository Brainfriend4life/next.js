//! Exposed for usage in `turbo-tasks-backend`

use std::io::{Read, Write};

use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;
use serde::{de::DeserializeOwned, Serialize};
use turbo_rcstr::RcStr;

pub fn to_vec<T>(config: &pot::Config, value: &T) -> anyhow::Result<(Vec<u8>, RcStrToLocalId)>
where
    T: Serialize,
{
    let mut vec = Vec::new();
    let ser_map = to_writer(config, value, &mut vec)?;
    Ok((vec, ser_map))
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

    let de_map = global_ids
        .0
        .into_iter()
        .map(query_db)
        .collect::<anyhow::Result<Vec<_>>>()?;

    turbo_rcstr::set_de_map(&de_map, || Ok(config.deserialize_from(&mut reader)?))
}
