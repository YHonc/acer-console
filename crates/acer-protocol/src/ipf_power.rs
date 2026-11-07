//! Narrow Intel IPF power primitive codec. No socket, DLL loading, or hardware calls.
//! Layout: Intel dptf 468699545442788999071d0a8ccb71b35332cdf1, default enum widths.
//! Offsets are relative to the END of each offset field, not the enclosing message.
const FRAME: usize = 16;
const FIXED: usize = 75;
const VOID: u32 = 24;
const POWER: u32 = 26;
const TIME: u32 = 31;
const UINT32: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access { ReadOnly, SupervisedWrite }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Limit { Pl1, Pl2 }
impl Limit { fn instance(self) -> u8 { match self { Self::Pl1 => 0, Self::Pl2 => 1 } } }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Operation {
    GetLimit, GetEnabled, GetTimeWindow,
    SetLimitMw(u32), SetEnabled(bool), SetTimeWindowMs(i64),
}
impl Operation {
    pub fn primitive(self) -> u32 { match self {
        Self::GetLimit => 38, Self::GetEnabled => 126, Self::GetTimeWindow => 339,
        Self::SetLimitMw(_) => 130, Self::SetEnabled(_) => 222, Self::SetTimeWindowMs(_) => 340,
    } }
    pub fn is_write(self) -> bool { matches!(self, Self::SetLimitMw(_) | Self::SetEnabled(_) | Self::SetTimeWindowMs(_)) }
    fn output(self) -> (u32, usize) { match self {
        Self::GetLimit => (POWER, 4), Self::GetEnabled => (UINT32, 4), Self::GetTimeWindow => (TIME, 8),
        _ => (VOID, 0),
    } }
}
/// Runtime-issued opaque handles. CPU/domain discovery must precede construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handles { pub esif: u64, pub participant: u64, pub domain: u64 }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub transaction: u64, pub clock_ticks: u64, pub clock_seconds: u64,
    pub handles: Handles, pub limit: Limit, pub operation: Operation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    WriteNotAuthorized, InvalidValue, InvalidHandle, Truncated,
    Header, Transaction, Identity, Layout, TypeOrLength, Remote(u32),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value { LimitMw(u32), Enabled(bool), TimeWindowMs(i64), Acknowledged }

fn put32(b: &mut [u8], p: usize, v: u32) { b[p..p+4].copy_from_slice(&v.to_le_bytes()); }
fn put64(b: &mut [u8], p: usize, v: u64) { b[p..p+8].copy_from_slice(&v.to_le_bytes()); }
fn u32_at(b: &[u8], p: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(b.get(p..p+4).ok_or(Error::Truncated)?.try_into().unwrap()))
}
fn u64_at(b: &[u8], p: usize) -> Result<u64, Error> {
    Ok(u64::from_le_bytes(b.get(p..p+8).ok_or(Error::Truncated)?.try_into().unwrap()))
}
fn append_data(b: &mut Vec<u8>, field: usize, typ: u32, data: &[u8]) {
    let pos = b.len();
    put32(b, field, (pos - (field + 4)) as u32);
    b.extend_from_slice(&typ.to_le_bytes());
    b.extend_from_slice(&(data.len() as u32).to_le_bytes()); // buf_len
    b.extend_from_slice(&(data.len() as u32).to_le_bytes()); // data_len
    b.extend_from_slice(&0u32.to_le_bytes()); // data starts immediately after descriptor
    b.extend_from_slice(data);
}

impl Request {
    /// Access is a local policy gate, NOT proof of a server ELEVATED session.
    /// A native caller must also bind/validate the actual session and dynamic ranges.
    pub fn encode(self, access: Access) -> Result<Vec<u8>, Error> {
        if self.operation.is_write() && access != Access::SupervisedWrite { return Err(Error::WriteNotAuthorized); }
        if [self.handles.esif, self.handles.participant, self.handles.domain].contains(&u64::MAX) { return Err(Error::InvalidHandle); }
        let (input_type, input) = match self.operation {
            Operation::SetLimitMw(0) => return Err(Error::InvalidValue),
            Operation::SetLimitMw(v) => (POWER, v.to_le_bytes().to_vec()),
            Operation::SetEnabled(v) => (UINT32, u32::from(v).to_le_bytes().to_vec()),
            Operation::SetTimeWindowMs(v) if v < 0 => return Err(Error::InvalidValue),
            Operation::SetTimeWindowMs(v) => (TIME, v.to_le_bytes().to_vec()),
            _ => (VOID, vec![]),
        };
        let mut body = vec![0u8; FIXED];
        put32(&mut body, 0, 1); // request
        // revision at 4 is UInt16 zero
        put32(&mut body, 6, 0x22);
        put64(&mut body, 10, self.transaction);
        put64(&mut body, 18, self.clock_ticks);
        put64(&mut body, 26, self.clock_seconds);
        put64(&mut body, 38, self.handles.esif);
        put64(&mut body, 46, self.handles.participant);
        put64(&mut body, 54, self.handles.domain);
        put32(&mut body, 70, self.operation.primitive());
        body[74] = self.limit.instance();
        append_data(&mut body, 62, input_type, &input);
        let (output_type, output_size) = self.operation.output();
        append_data(&mut body, 66, output_type, &vec![0; output_size]);
        Ok(frame(body))
    }

    /// Validate a complete ESIF frame after WebSocket reassembly, not arbitrary stream chunks.
    pub fn decode(self, bytes: &[u8]) -> Result<Value, Error> {
        if bytes.len() < FRAME + FIXED { return Err(Error::Truncated); }
        if bytes[..4] != [0xe5, 0xd8, 16, 0] || u32_at(bytes, 4)? != 0x01000000
            || u32_at(bytes, 8)? != 0x43505249 || u32_at(bytes, 12)? as usize != bytes.len()-FRAME {
            return Err(Error::Header);
        }
        let b = &bytes[FRAME..];
        if u32_at(b, 0)? != 2 || b[4..6] != [0, 0] || u32_at(b, 6)? != 0x22 { return Err(Error::Header); }
        if u64_at(b, 10)? != self.transaction { return Err(Error::Transaction); }
        if u64_at(b, 38)? != self.handles.esif || u64_at(b, 46)? != self.handles.participant
            || u64_at(b, 54)? != self.handles.domain || b[74] != self.limit.instance()
            || u32_at(b, 70)? != self.operation.primitive() { return Err(Error::Identity); }
        let rc = u32_at(b, 34)?;
        if rc != 0 { return Err(Error::Remote(rc)); }
        let relative = u32_at(b, 66)?;
        let (typ, len) = self.operation.output();
        if relative == u32::MAX {
            return if typ == VOID { Ok(Value::Acknowledged) } else { Err(Error::Layout) };
        }
        let pos = 70usize.checked_add(relative as usize).ok_or(Error::Layout)?;
        if pos < FIXED || pos.checked_add(16).ok_or(Error::Layout)? > b.len() { return Err(Error::Layout); }
        let actual_len = u32_at(b, pos+8)? as usize;
        // EsifDataTime uses an Int64 buffer, while esif_data_type_size(TIME)
        // in the same SDK describes UInt32. Accept both explicitly, never pad
        // arbitrary short data or reinterpret a different type as TIME.
        let valid_len = if typ == TIME { actual_len == 4 || actual_len == 8 } else { actual_len == len };
        if u32_at(b, pos)? != typ || !valid_len || (u32_at(b, pos+4)? as usize) < actual_len {
            return Err(Error::TypeOrLength);
        }
        let data = (pos+16).checked_add(u32_at(b, pos+12)? as usize).ok_or(Error::Layout)?;
        let end = data.checked_add(actual_len).ok_or(Error::Layout)?;
        let raw = b.get(data..end).ok_or(Error::Layout)?;
        match self.operation {
            Operation::GetLimit => Ok(Value::LimitMw(u32::from_le_bytes(raw.try_into().unwrap()))),
            Operation::GetEnabled => Ok(Value::Enabled(u32::from_le_bytes(raw.try_into().unwrap()) > 0)),
            Operation::GetTimeWindow => {
                let ms = if raw.len() == 4 { i64::from(u32::from_le_bytes(raw.try_into().unwrap())) }
                    else { i64::from_le_bytes(raw.try_into().unwrap()) };
                if ms < 0 { Err(Error::InvalidValue) } else { Ok(Value::TimeWindowMs(ms)) }
            }
            _ => Ok(Value::Acknowledged),
        }
    }
}
fn frame(body: Vec<u8>) -> Vec<u8> {
    let mut out = vec![0xe5, 0xd8, 16, 0];
    out.extend_from_slice(&0x01000000u32.to_le_bytes());
    out.extend_from_slice(&0x43505249u32.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body); out
}

#[cfg(test)] mod tests {
    use super::*;
    fn request(op: Operation) -> Request { Request { transaction: 0x1122334455667788, clock_ticks: 0, clock_seconds: 0,
        handles: Handles { esif: 11, participant: 22, domain: 33 }, limit: Limit::Pl1, operation: op } }
    // Independently built response fixture: fixed header, absent const request, response
    // descriptor directly at 75 so its pointer is 75-(66+4)=5. Not an encode/decode roundtrip.
    fn response(r: Request, typ: u32, raw: &[u8]) -> Vec<u8> {
        let mut b = vec![0; 75]; put32(&mut b, 0, 2); put32(&mut b, 6, 34); put64(&mut b, 10, r.transaction);
        put64(&mut b, 38, 11); put64(&mut b, 46, 22); put64(&mut b, 54, 33);
        put32(&mut b, 62, u32::MAX); put32(&mut b, 66, 5); put32(&mut b, 70, r.operation.primitive()); b[74]=r.limit.instance();
        for n in [typ, raw.len() as u32, raw.len() as u32, 0] { b.extend_from_slice(&n.to_le_bytes()); }
        b.extend_from_slice(raw); frame(b)
    }
    #[test] fn get_wire_has_field_relative_offsets_and_void_request() {
        let b = request(Operation::GetLimit).encode(Access::ReadOnly).unwrap();
        assert_eq!(&b[FRAME+62..FRAME+70], &[9,0,0,0,21,0,0,0]);
        assert_eq!(&b[FRAME+75..FRAME+91], &[24,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);
        assert_eq!(b.len(), 16+75+16+16+4); assert_eq!(u32_at(&b,12),Ok(111));
    }
    #[test] fn setters_are_exact_and_require_native_write_policy() {
        for (op,id,typ,n) in [(Operation::SetLimitMw(45000),130,POWER,4),(Operation::SetEnabled(false),222,UINT32,4),(Operation::SetTimeWindowMs(28000),340,TIME,8)] {
            let r=request(op); assert_eq!(r.encode(Access::ReadOnly),Err(Error::WriteNotAuthorized));
            let b=r.encode(Access::SupervisedWrite).unwrap();
            assert_eq!(u32_at(&b,FRAME+70),Ok(id)); assert_eq!(u32_at(&b,FRAME+75),Ok(typ));
            assert_eq!(u32_at(&b,FRAME+66),Ok(21+n));
        }
    }
    #[test] fn typed_readbacks_and_remote_errors() {
        let r=request(Operation::GetLimit); let mut b=response(r,POWER,&45000u32.to_le_bytes());
        assert_eq!(r.decode(&b),Ok(Value::LimitMw(45000)));
        put32(&mut b,FRAME+34,3703); put32(&mut b,FRAME+66,u32::MAX);
        assert_eq!(r.decode(&b),Err(Error::Remote(3703)));
        let r=request(Operation::GetTimeWindow);
        assert_eq!(r.decode(&response(r,TIME,&28000i64.to_le_bytes())),Ok(Value::TimeWindowMs(28000)));
        assert_eq!(r.decode(&response(r,TIME,&28000u32.to_le_bytes())),Ok(Value::TimeWindowMs(28000)));
        assert_eq!(r.decode(&response(r,TIME,&[0;2])),Err(Error::TypeOrLength));
    }
    #[test] fn mismatched_transactions_handles_and_offsets_are_rejected() {
        let r=request(Operation::GetLimit); let original=response(r,POWER,&45000u32.to_le_bytes());
        for (offset,value,expected) in [(10,0,Error::Transaction),(46,0,Error::Identity),(66,0,Error::Layout),(66,u32::MAX-1,Error::Layout)] {
            let mut b=original.clone();put32(&mut b,FRAME+offset,value);assert_eq!(r.decode(&b),Err(expected));
        }
        for n in 0..original.len() { assert!(r.decode(&original[..n]).is_err()); }
    }
    #[test] fn malformed_type_length_and_no_output_never_become_zero_readings() {
        let r=request(Operation::GetLimit);assert_eq!(r.decode(&response(r,UINT32,&0u32.to_le_bytes())),Err(Error::TypeOrLength));
        let mut b=response(r,POWER,&0u32.to_le_bytes());put32(&mut b,FRAME+66,u32::MAX);assert_eq!(r.decode(&b),Err(Error::Layout));
        assert_eq!(request(Operation::SetTimeWindowMs(-1)).encode(Access::SupervisedWrite),Err(Error::InvalidValue));
        assert_eq!(request(Operation::SetLimitMw(0)).encode(Access::SupervisedWrite),Err(Error::InvalidValue));
    }
}
