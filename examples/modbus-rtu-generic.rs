use coriolis::core::modbus::*;
//{FW_REG_COUNT, decode_any_reg, decode_generic_reg};
use std::collections::HashMap;
use tokio_modbus::slave;
//#[cfg(feature = "modbus-rtu")]
pub fn main() {
    use chrono::{DateTime, Utc};
    use env_logger::Builder as LoggerBuilder;
    use futures::{future::Either, Future, Stream};
    use std::{cell::RefCell, env, io::Error, rc::Rc, time::Duration};
    use stream_cancel::{StreamExt, Tripwire};
    use tokio::timer::Interval;
    use tokio_core::reactor::{Core, Handle};
    use tokio_modbus::prelude::{client::util::*, *};

    use coriolis::{buildmap::build_hashmap, modbus, *};

    use csv::{Reader, Writer, WriterBuilder};

    use std::fs::File;
    use std::fs::OpenOptions;
    // Open a file to write the CSV data to
    let file = File::create("data.csv").expect("hay problemo");
    // Build the HashMap from CSV
    let path = String::from("ModbusMap.csv");
    //let my_hmap: HashMap<u16, String> = build_hashmap(&path);

    let mut logger_builder = LoggerBuilder::new();
    logger_builder.filter_level(log::LevelFilter::Info);
    if env::var("RUST_LOG").is_ok() {
        let rust_log_var = &env::var("RUST_LOG").unwrap();
        println!("Parsing RUST_LOG={}", rust_log_var);
        logger_builder.parse_filters(rust_log_var);
    }
    logger_builder.init();

    let mut core = Core::new().unwrap();

    #[derive(Debug, Clone)]
    struct ContextConfig {
        handle: Handle,
        tty_path: String,
    }

    impl NewContext for ContextConfig {
        fn new_context(&self) -> Box<dyn Future<Item = client::Context, Error = Error>> {
            Box::new(modbus::rtu::connect_path(&self.handle, &self.tty_path))
        }
    }

    #[derive(Debug, Clone)]
    struct SlaveConfig {
        slave: Slave,
        cycle_time: Duration,
        timeout: Duration,
        read_index: usize,
        regs: Vec<u16>,
        hmap: HashMap<u16, String>,
    }
    impl SlaveConfig {
        fn next(&mut self) {
            self.read_index = (self.read_index + 1) % self.regs.len();
        }
        fn add_regs(&mut self, regs: Vec<u16>) {
            for reg in regs {
                self.regs.push(reg);
            }
        }
    }
    // TODO: Parse parameters and options from command-line arguments
    //read the config file
    let new_config = setup::read_config();
    //unpack the config here
    let com_list = new_config.ComPort;
    let mb_addr: Slave = Slave(new_config.ModbusAddress);
    let interval = new_config.cycle_time;
    let regs = new_config.Regs;
    let timeout = new_config.timeout;
    let context_config = ContextConfig {
        handle: core.handle(),
        tty_path: com_list[0].to_owned(),
        //tty_path: "COM9".to_owned(),
    };

    let mut slave_config = SlaveConfig {
        slave: mb_addr,
        cycle_time: Duration::from_millis(interval),
        timeout: Duration::from_millis(timeout),
        read_index: 0,
        regs: Vec::new(),
        hmap: build_hashmap(&path),
    };
    // TODO: Get these regs from user input

    //let regs: Vec<u16> = vec![103, 95, 154, 119];
    //let regs: Vec<u16> = vec![5523, 119, 121, 126];
    slave_config.add_regs(regs);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Measurement<T> {
        ts: DateTime<Utc>,
        val: T,
        reg: u16,
    }

    impl<T> Measurement<T> {
        pub fn new(val: T, reg: u16) -> Self {
            Self {
                ts: Utc::now(),
                val,
                reg,
            }
        }
    }
    //removed Copy to accomodate generic<String>,
    //should these be reg names? or reg types? (Coil/Reg/Long/Float/Ascii)
    #[derive(Debug, Default, Clone, PartialEq)]
    struct Measurements {
        generic: Option<Measurement<Generic>>,
        register: Option<Measurement<Register>>,
        float: Option<Measurement<Float>>,
    }

    // Only a single slave sensor is used for demonstration purposes here.
    // A typical application will use multiple slaves that all share
    // the same Modbus environment, RTU client context and bus wiring,
    // i.e. multiple sensors and actuators are all connected to a single
    // serial port.
    struct ControlLoop {
        // Only shared with the single proxy and otherwise unused within the
        // control loop. Just to demonstrate how to share the Modbus context
        // and how to recover from communication errors by reconnecting.
        _shared_context: Rc<RefCell<SharedContext>>,
        config: SlaveConfig,
        proxy: modbus::SlaveProxy,
        measurements: Measurements,
    }

    impl ControlLoop {
        pub fn new(config: SlaveConfig, new_context: Box<dyn NewContext>) -> Self {
            let shared_context = Rc::new(RefCell::new(SharedContext::new(None, new_context)));
            let proxy = modbus::SlaveProxy::new(config.slave, Rc::clone(&shared_context));
            Self {
                _shared_context: shared_context,
                config,
                proxy,
                measurements: Default::default(),
            }
        }

        fn reconnect(&self) -> impl Future<Item = (), Error = Error> {
            self.proxy.reconnect()
        }

        //lets make this handle any reg type
        pub fn measure_any(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
            let reg_start = self.config.regs[self.config.read_index];
            //use HashMap lookup to get reg_count and reg_type
            let map_value = self.config.hmap.get(&reg_start).unwrap().as_str();
            let reg_type: char = map_value.chars().nth(0).unwrap();
            //cut reg_count/2 if reg_type == 'A'
            let reg_count = count_calc(&reg_type, map_value[1..].parse::<u16>().unwrap());
            println!("reg: {:?}", &reg_start);
            self.proxy
                .read_generic(
                    Some(self.config.timeout),
                    reg_start - 1,
                    reg_count,
                    reg_type,
                )
                //move into closure and do the decode for each type
                .then(move |res| match res {
                    Ok(val) => {
                        if reg_type == 'A' {
                            println!("got a 'A'");
                            let d = decode_generic_reg(val);
                            match d {
                                Ok(res) => self.measurements.generic = Some(Measurement::new(res, reg_start)),
                                Err(e) => println!("decode error {:?}", e),
                            }
                            Ok(self)
                        } else if reg_type == 'U' {
                            println!("got a 'U'");
                            let d = decode_u_reg(val);
                            match d {
                                Ok(res) => self.measurements.register = Some(Measurement::new(res, reg_start)),
                                Err(e) => println!("decode error {:?}", e),
                            }
                            Ok(self)
                        } else if reg_type == 'F' {
                            println!("got a 'F'");
                            let d = decode_f_reg(val);
                            match d {
                                Ok(res) => self.measurements.float = Some(Measurement::new(res, reg_start)),
                                Err(e) => println!("decode error {:?}", e),
                            }
                            Ok(self)
                        } else {
                            println!("got a 'something else'");
                            //self.measurements.vec = None;
                            Ok(self)
                        }
                    }
                    Err(err) => {
                        println!("error in read_generic");
                        //self.measurements.vec = None;
                        //Ok(self)
                        Err((err, self))
                    }
                })
        }

        pub fn recover_after_error(&self, err: &Error) -> impl Future<Item = (), Error = ()> {
            log::warn!("Reconnecting after error: {}", err);
            self.reconnect().or_else(|err| {
                log::error!("Failed to reconnect: {}", err);
                // Continue and don't leave/terminate the control loop!
                Ok(())
            })
        }

        pub fn broadcast_slave(&self) -> impl Future<Item = (), Error = Error> {
            self.proxy.broadcast_slave()
        }
    }

    log::info!("Connecting: {:?}", context_config);
    //maybe here? turn context_config into ctx
    let ctrl_loop = ControlLoop::new(slave_config, Box::new(context_config));

    //ctrl_loop.config.reg_start = 0xf6;
    //ctrl_loop.config.reg_count = 0x02;
    core.run(ctrl_loop.reconnect()).unwrap();

    let broadcast_slave = false;
    if broadcast_slave {
        log::info!(
            "Resetting Modbus slave address to {:?}",
            ctrl_loop.proxy.slave()
        );
        core.run(ctrl_loop.broadcast_slave()).unwrap();
    }
    fn write_to_csv(data: Measurements) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(String::from("data.csv"))?;
        let mut wtr: Writer<File> = csv::WriterBuilder::new()
            .has_headers(true)
            .from_writer(file);
        //nn to modify for all measurement types
        let generic = vec![
            data.register.as_ref().unwrap().ts.to_string(),
            data.register.as_ref().unwrap().val.to_string(),
            data.register.as_ref().unwrap().reg.to_string(),
            
        ];
        wtr.write_record(generic)?;

        wtr.flush()?;
        Ok(())
    }

    let (_trigger, tripwire) = Tripwire::new();
    let cycle_interval = Interval::new_interval(ctrl_loop.config.cycle_time);
    let ctrl_loop_task = cycle_interval
        .map_err(|err| {
            log::error!("Aborting control loop after timer error: {:?}", err);
        })
        .take_until(tripwire)
        .fold(ctrl_loop, |ctrl_loop, _event| {
            // Asynchronous chain of measurements. The control loop
            // is consumed and returned upon each step to update the
            // measurement after reading a new value asynchronously.
            futures::future::ok(ctrl_loop)
                .and_then(ControlLoop::measure_any)
                .then(|res| match res {
                    Ok(mut ctrl_loop) => {
                        //write_to_csv(ctrl_loop.measurements.clone());
                        //for Some(measurement)

                        //println!("Some {:?}", ctrl_loop.measurements);
                        log::info!("{:?}", ctrl_loop.measurements);
                        ctrl_loop.config.next(); //increment the modbus reg read index
                        Either::A(futures::future::ok(ctrl_loop))
                    }
                    Err((err, mut ctrl_loop)) => {
                        log::info!("{:?}", ctrl_loop.measurements);
                        ctrl_loop.config.next();
                        Either::B(ctrl_loop.recover_after_error(&err).map(|()| ctrl_loop))
                    }
                })
        });

    core.run(ctrl_loop_task).unwrap();
}

/*#[cfg(not(feature = "modbus-rtu"))]
pub fn main() {
    println!("feature `modbus-rtu` is required to run this example");
    std::process::exit(1);
}*/
