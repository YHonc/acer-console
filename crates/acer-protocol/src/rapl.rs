//! Read-only Intel package RAPL decoding. These fields describe configured limits,
//! not measured consumption or permission to write a CPU register.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerLimit {
    pub watts: f64,
    pub enabled: bool,
    pub clamp: bool,
    pub window_seconds: f64,
    pub raw_power: u16,
    pub raw_window: u8,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageLimits {
    // Hex strings preserve all 64 bits across JavaScript consumers.
    pub raw_units_hex: String,
    pub raw_limits_hex: String,
    pub power_unit_watts: f64,
    pub time_unit_seconds: f64,
    pub locked: bool,
    pub pl1: PowerLimit,
    pub pl2: PowerLimit,
}

/// Caller must establish CPU support and successful exact-length reads of
/// MSR_RAPL_POWER_UNIT (0x606) and MSR_PKG_POWER_LIMIT (0x610).
/// None means unavailable, never a fabricated zero-watt limit.
pub fn decode(units: Option<u64>, limits: Option<u64>) -> Option<PackageLimits> {
    let (units, limits)=(units?, limits?);
    let power_unit=2f64.powi(-((units & 0xF) as i32));
    let time_unit=2f64.powi(-(((units >> 16) & 0xF) as i32));
    let field=|word:u64| {
        let raw_power=(word & 0x7FFF) as u16;
        let raw_window=((word >> 17) & 0x7F) as u8;
        PowerLimit {watts:f64::from(raw_power)*power_unit,enabled:word & (1<<15)!=0,clamp:word & (1<<16)!=0,
            window_seconds:2f64.powi((raw_window & 31) as i32)*(1.0+f64::from(raw_window >> 5)/4.0)*time_unit,
            raw_power,raw_window}
    };
    Some(PackageLimits {raw_units_hex:format!("0x{units:016X}"),raw_limits_hex:format!("0x{limits:016X}"),
        power_unit_watts:power_unit,time_unit_seconds:time_unit,locked:limits & (1u64<<63)!=0,pl1:field(limits),pl2:field(limits>>32)})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn synthetic_units_limits_and_windows() {
        // Synthetic fixture only: 1/8 W and 1/1024 second units.
        let units=3 | (10<<16);
        let pl1=520u64 | (1<<15) | (1<<16) | ((14u64 | (3<<5))<<17);
        let pl2=1256u64 | (1<<15) | (10<<17);
        let raw=pl1 | (pl2<<32) | (1u64<<63);
        let value=decode(Some(units),Some(raw)).unwrap();
        assert_eq!(value.pl1.watts,65.0);assert_eq!(value.pl2.watts,157.0);
        assert_eq!(value.pl1.window_seconds,28.0);assert_eq!(value.pl2.window_seconds,1.0);
        assert!(value.pl1.enabled&&value.pl1.clamp&&value.pl2.enabled&&value.locked);
        assert!(!value.pl2.clamp);
        assert_eq!(value.raw_limits_hex,format!("0x{raw:016X}"));
    }
    #[test] fn unavailable_is_distinct_from_disabled_and_zero() {
        assert!(decode(None,Some(0)).is_none());assert!(decode(Some(0),None).is_none());
        let value=decode(Some(0),Some(0)).unwrap();assert_eq!(value.pl1.watts,0.0);assert!(!value.pl1.enabled);assert!(!value.locked);
    }
    #[test] fn reserved_fields_do_not_change_decoded_values() {
        let baseline=decode(Some(3),Some(0)).unwrap();
        let reserved=(0xFFu64<<24)|(0x7Fu64<<56);
        let value=decode(Some(3 | (0xFFFFu64<<32)),Some(reserved)).unwrap();
        assert_eq!(baseline.pl1,value.pl1);assert_eq!(baseline.pl2,value.pl2);assert!(!value.locked);
    }
}
