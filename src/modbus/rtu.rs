use super::*;

use crate::core::modbus::rtu::*;

use futures::{future, Future};
use serialport::SerialPort;
use std::{io::Error, path::Path, time::Duration};
use tokio_core::reactor::{Core, Handle};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_modbus::client::{rtu::connect_slave, Context as ClientContext};
use tokio_modbus::prelude::*;
use tokio_serial::{Serial, SerialPortSettings};

pub const SERIAL_PORT_SETTINGS: SerialPortSettings = SerialPortSettings {
    baud_rate: BAUD_RATE,
    data_bits: DATA_BITS,
    stop_bits: STOP_BITS,
    parity: PARITY,
    flow_control: FLOW_CONTROL,
    timeout: Duration::from_secs(0),
};

pub fn connect<T: AsyncRead + AsyncWrite + 'static>(
    handle: &Handle,
    transport: T,
) -> impl Future<Item = ClientContext, Error = Error> {
    connect_slave(handle, transport, BROADCAST_SLAVE)
}

pub fn connect_path(
    handle: &Handle,
    path: impl AsRef<Path>,
) -> Box<dyn Future<Item = ClientContext, Error = Error>> {
    log::info!("Connecting to serial port {}", path.as_ref().display());
    match Serial::from_path_with_handle(path, &SERIAL_PORT_SETTINGS, handle.new_tokio_handle()) {
        Ok(mut serial) => {
            //set the DTR pin
            serial.write_data_terminal_ready(true).unwrap();
            Box::new(connect(handle, serial))
        }
        Err(err) => Box::new(future::err(err)),
    }
}
