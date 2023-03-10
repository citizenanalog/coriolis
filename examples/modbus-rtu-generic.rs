use coriolis::core::modbus::FW_REG_COUNT;
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
    let context_config = ContextConfig {
        handle: core.handle(),
        //tty_path: "/dev/ttyACM0".to_owned(),
        tty_path: "COM9".to_owned(),
    };
    let mut slave_config = SlaveConfig {
        slave: Slave::min_device(),
        cycle_time: Duration::from_millis(1000),
        timeout: Duration::from_millis(500),
        read_index: 0,
        regs: Vec::new(),
        hmap: build_hashmap(&path),
    };
    // TODO: Get these regs from user input
    let regs: Vec<u16> = vec![103, 95, 154, 119];
    //let regs: Vec<u16> = vec![5523, 119, 121, 126];
    slave_config.add_regs(regs);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Measurement<T> {
        ts: DateTime<Utc>,
        val: T,
    }

    impl<T> Measurement<T> {
        pub fn new(val: T) -> Self {
            Self {
                ts: Utc::now(),
                val,
            }
        }
    }
    //removed Copy to accomodate generic<String>,
    //should these be reg names? or reg types? (Coil/Reg/Long/Float/Ascii)
    #[derive(Debug, Default, Clone, PartialEq)]
    struct Measurements {
        temperature: Option<Measurement<Temperature>>,
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

        pub fn measure_temperature(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
            self.proxy
                .read_temperature(Some(self.config.timeout))
                .then(move |res| match res {
                    Ok(val) => {
                        self.measurements.temperature = Some(Measurement::new(val));

                        Ok(self)
                    }
                    Err(err) => Err((err, self)),
                })
        }

        //nn to have measure_any, measure_float, measure_reg
        pub fn measure_any(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
            let reg_start = self.config.regs[self.config.read_index];
            let plus_one = reg_start + 1; //reg offset by 1
                                          //use HashMap lookup to get reg_count and reg_type
            let map_value = self.config.hmap.get(&plus_one).unwrap().as_str();
            let reg_type: char = map_value.chars().nth(0).unwrap();
            let reg_count = map_value[1..].parse::<u16>().unwrap();
            //match on reg_type to call corrected associated type (Register/Long/Float/Generic)
                    self.proxy
                .read_generic(Some(self.config.timeout), reg_start, reg_count, reg_type)
                .then(move |res| match res {
                    Ok(val) => {
                        if reg_type == 'A' {
                            //println!("got a 'A'");
                            self.measurements.generic = Some(Measurement::new(val));
                            Ok(self)
                        } else {
                            //println!("got a 'something else'");
                            self.measurements.generic = None;
                            Ok(self)
                        }
                        
                    }
                    Err(_err) => {
                        //println!("got a 'something else'");
                        self.measurements.generic = None;
                        Ok(self)
                        //Err((err, self))
                    }
                })
            
                
            }
            pub fn measure_float(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
                let reg_start = self.config.regs[self.config.read_index];
                let plus_one = reg_start + 1; //reg offset by 1
                                              //use HashMap lookup to get reg_count and reg_type
                let map_value = self.config.hmap.get(&plus_one).unwrap().as_str();
                let reg_type: char = map_value.chars().nth(0).unwrap();
                let reg_count = map_value[1..].parse::<u16>().unwrap();
                //match on reg_type to call corrected associated type (Register/Long/Float/Generic)
                        self.proxy
                    .read_float(Some(self.config.timeout), reg_start, reg_count, reg_type)
                    .then(move |res| match res {
                        Ok(val) => {
                            if reg_type == 'F' {
                               // println!("got a 'F'");
                                self.measurements.float = Some(Measurement::new(val));
                                Ok(self)
                            } else {
                                //println!("got a 'something else'");
                                self.measurements.float = None;
                                Ok(self)
                            }
                        }
                        Err(_err) => {
                            //println!("got a 'something else'");
                                self.measurements.float = None;
                                Ok(self)
                        }
                    })
                
                    
                }

                pub fn measure_reg(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
                    let reg_start = self.config.regs[self.config.read_index];
                    let plus_one = reg_start + 1; //reg offset by 1
                                                  //use HashMap lookup to get reg_count and reg_type
                    let map_value = self.config.hmap.get(&plus_one).unwrap().as_str();
                    let reg_type: char = map_value.chars().nth(0).unwrap();
                    let reg_count = map_value[1..].parse::<u16>().unwrap();
                    //match on reg_type to call corrected associated type (Register/Long/Float/Generic)
                            self.proxy
                        .read_register(Some(self.config.timeout), reg_start, reg_count, reg_type)
                        .then(move |res| match res {
                            Ok(val) => {
                        if reg_type == 'U' {
                            //println!("got a 'U'");
                            self.measurements.register = Some(Measurement::new(val));
                            Ok(self)
                        } else {
                            //println!("got a 'something else'");
                            self.measurements.register = None;
                            Ok(self)
                        }
                            }
                            Err(_err) => {
                                //println!("got a 'something else'");
                            self.measurements.register = None;
                            Ok(self)
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
    let mut ctrl_loop = ControlLoop::new(slave_config, Box::new(context_config));
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
            data.generic.as_ref().unwrap().ts.to_string(),
            data.generic.as_ref().unwrap().val.to_string(),
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
            // get the reg type here?
            futures::future::ok(ctrl_loop)
                .and_then(ControlLoop::measure_any)
                .and_then(ControlLoop::measure_float)
                .and_then(ControlLoop::measure_reg)
                .then(|res| match res {
                    Ok(mut ctrl_loop) => {
                        //write_to_csv(ctrl_loop.measurements.clone());
                        //for Some(measurement)
                        
                        println!("Some {:?}", ctrl_loop.measurements);
                        log::info!("{:?}", ctrl_loop.measurements.clone());
                        ctrl_loop.config.next(); //increment the modbus reg read index
                        Either::A(futures::future::ok(ctrl_loop))
                    }
                    Err((err, mut ctrl_loop)) => {
                        log::info!("{:?}", ctrl_loop.measurements.clone());
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
