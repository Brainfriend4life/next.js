//! Exposed for usage in `turbo-tasks-backend`

use std::io::Write;

use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use turbo_rcstr::RcStr;

#[derive(Serialize, Deserialize)]
struct Data(Vec<u8>, IndexSet<RcStr, FxBuildHasher>);

pub fn to_vec<T>(config: &pot::Config, value: &T) -> pot::Result<Vec<u8>>
where
    T: Serialize,
{
    let mut vec = Vec::new();
    to_writer(config, value, &mut vec)?;
    Ok(vec)
}

pub fn to_writer<T, W>(config: &pot::Config, value: &T, writer: W) -> pot::Result<()>
where
    T: Serialize,
    W: Write,
{
    let (result, ser_map) = turbo_rcstr::set_ser_map(|| config.serialize(value));
    let value = result?;
    let data = Data(value, ser_map);
    config.serialize_into(&data, writer)
}

pub fn from_slice<T>(config: &pot::Config, slice: &[u8]) -> pot::Result<T>
where
    T: DeserializeOwned,
{
    let data: Data = config.deserialize(slice)?;

    turbo_rcstr::set_de_map(&data.1, || config.deserialize(&data.0))
}
