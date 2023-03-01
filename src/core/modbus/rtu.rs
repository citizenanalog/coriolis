use serialport::{DataBits, FlowControl, Parity, StopBits};

pub const BAUD_RATE: u32 = 38400;
pub const DATA_BITS: DataBits = DataBits::Eight;
pub const STOP_BITS: StopBits = StopBits::One;
pub const PARITY: Parity = Parity::None;
pub const FLOW_CONTROL: FlowControl = FlowControl::None;
