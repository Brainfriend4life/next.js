//! Exposed for usage in `turbo-tasks-backend`

use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use turbo_rcstr::RcStr;

#[derive(Serialize, Deserialize)]
struct Data(Vec<u8>, IndexSet<RcStr, FxBuildHasher>);

pub fn to_vec<T>(value: &T) -> pot::Result<Vec<u8>>
where
    T: Serialize,
{
    let (result, ser_map) = turbo_rcstr::set_ser_map(|| pot::to_vec(value));
    let value = result?;
    let data = Data(value, ser_map);
    pot::to_vec(&data)
}

pub fn from_slice<T>(slice: &[u8]) -> pot::Result<T>
where
    T: DeserializeOwned,
{
    let data: Data = pot::from_slice(slice)?;

    turbo_rcstr::set_de_map(&data.1, || pot::from_slice(&data.0))
}
