//! Strict PPCC capability decoding; no synthetic PL2/3/4 fallback rows.
//! Intel EsifDataBinaryPpccPackage: 12-byte revision variant, 72-byte rows.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub index: u32, pub min_mw: u32, pub max_mw: u32,
    pub time_bounds_ms: Option<(u32,u32)>, pub step_mw: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error { Length, VariantType, Revision(u64), Index, Duplicate, PowerRange, TimeRange, Overflow }
fn integer(data: &[u8]) -> Result<u64,Error> {
    if data.len()!=12 { return Err(Error::Length); }
    if u32::from_le_bytes(data[..4].try_into().unwrap())!=4 { return Err(Error::VariantType); }
    Ok(u64::from_le_bytes(data[4..12].try_into().unwrap()))
}
pub fn decode(data: &[u8]) -> Result<Vec<Capability>,Error> {
    if data.len()<84 || (data.len()-12)%72!=0 || data.len()>12+4*72 { return Err(Error::Length); }
    let revision=integer(&data[..12])?;
    if revision!=2 { return Err(Error::Revision(revision)); }
    let mut result:Vec<Capability>=Vec::new();
    for row in data[12..].chunks_exact(72) {
        let mut values=[0u32;6];
        for (slot,variant) in values.iter_mut().zip(row.chunks_exact(12)) {
            *slot=u32::try_from(integer(variant)?).map_err(|_|Error::Overflow)?;
        }
        let [index,min_mw,max_mw,tmin,tmax,step]=values;
        if index>3 { return Err(Error::Index); }
        if result.iter().any(|c|c.index==index) { return Err(Error::Duplicate); }
        if min_mw==0 || min_mw==u32::MAX || max_mw==u32::MAX || min_mw>max_mw { return Err(Error::PowerRange); }
        let time_bounds_ms=if tmin==u32::MAX || tmax==u32::MAX { None }
            else if tmin>tmax { return Err(Error::TimeRange); } else { Some((tmin,tmax)) };
        result.push(Capability {index,min_mw,max_mw,time_bounds_ms,step_mw:if step==0 || step==u32::MAX {None} else {Some(step)}});
    }
    Ok(result)
}
#[cfg(test)] mod tests {
    use super::*;
    fn fixture(rows:&[[u32;6]])->Vec<u8> {
        let mut data=Vec::new();
        for value in std::iter::once(2u64).chain(rows.iter().flatten().map(|v|u64::from(*v))) {
            data.extend_from_slice(&4u32.to_le_bytes());data.extend_from_slice(&value.to_le_bytes());
        }data
    }
    #[test]fn explicit_rows_preserve_units_without_generating_missing_limits(){
        let caps=decode(&fixture(&[[0,10000,55000,0,28000,1000]])).unwrap();
        assert_eq!(caps.len(),1);assert_eq!(caps[0].max_mw,55000);
        assert_eq!(caps[0].time_bounds_ms,Some((0,28000)));assert_eq!(caps[0].step_mw,Some(1000));
        assert_eq!(decode(&fixture(&[[1,10000,80000,u32::MAX,u32::MAX,0]])).unwrap()[0].time_bounds_ms,None);
    }
    #[test]fn malformed_unknown_and_conflicting_capabilities_are_rejected(){
        let row=[0,10000,55000,0,28000,1000];let data=fixture(&[row]);
        assert_eq!(decode(&data[..data.len()-1]),Err(Error::Length));
        assert_eq!(decode(&fixture(&[row,row])),Err(Error::Duplicate));
        assert_eq!(decode(&fixture(&[[0,60000,55000,0,28000,1000]])),Err(Error::PowerRange));
        let mut bad=data.clone();bad[0]=3;assert_eq!(decode(&bad),Err(Error::VariantType));
        bad=data.clone();bad[4]=9;assert_eq!(decode(&bad),Err(Error::Revision(9)));
        bad=data;bad[32..36].copy_from_slice(&1u32.to_le_bytes());assert_eq!(decode(&bad),Err(Error::Overflow));
    }
}
