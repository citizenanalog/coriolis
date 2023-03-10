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

    use coriolis::{modbus, *};

    use csv::{Reader, Writer, WriterBuilder};

    use std::fs::File;
    use std::fs::OpenOptions;
    // Open a file to write the CSV data to
    let file = File::create("data.csv").expect("hay problemo");

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
    };

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
    };

    // TODO: Parse parameters and options from command-line arguments
    let context_config = ContextConfig {
        handle: core.handle(),
        //tty_path: "/dev/ttyACM0".to_owned(),
        tty_path: "COM9".to_owned(),
    };
    let slave_config = SlaveConfig {
        slave: Slave::min_device(),
        cycle_time: Duration::from_millis(1000),
        timeout: Duration::from_millis(500),
    };

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
        //water_content: Option<Measurement<VolumetricWaterContent>>,
        //permittivity: Option<Measurement<RelativePermittivity>>,
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
    };

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

        pub fn measure_generic(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
            self.proxy
                .read_generic(Some(self.config.timeout))
                .then(move |res| match res {
                    Ok(val) => {
                        self.measurements.generic = Some(Measurement::new(val));

                        Ok(self)
                    }
                    Err(err) => Err((err, self)),
                })
        }

        pub fn read_any(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
            self.proxy
                .read_generic(Some(self.config.timeout))
                .then(move |res| match res {
                    Ok(val) => {
                        self.measurements.generic = Some(Measurement::new(val));

                        Ok(self)
                    }
                    Err(err) => Err((err, self)),
                })
        }

        /*
                pub fn measure_water_content(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
                    self
                        .proxy
                        .read_water_content(Some(self.config.timeout))
                        .then(move |res| match res {
                            Ok(val) => {
                                self.measurements.water_content = Some(Measurement::new(val));
                                Ok(self)
                            }
                            Err(err) => Err((err, self)),
                        })
                }

                pub fn measure_permittivity(mut self) -> impl Future<Item = Self, Error = (Error, Self)> {
                    self
                        .proxy
                        .read_permittivity(Some(self.config.timeout))
                        .then(move |res| match res {
                            Ok(val) => {
                                self.measurements.permittivity = Some(Measurement::new(val));
                                Ok(self)
                            }
                            Err(err) => Err((err, self)),
                        })
                }
        */
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
    let ctrl_loop = ControlLoop::new(slave_config, Box::new(context_config));
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
        let temp = vec![
            data.temperature.unwrap().ts.to_string(),
            data.temperature.unwrap().val.to_string(),
        ];
        wtr.write_record(temp)?;

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
                //.and_then(ControlLoop::measure_temperature)
                .and_then(ControlLoop::measure_generic)
                //.and_then(ControlLoop::measure_water_content)
                //.and_then(ControlLoop::measure_permittivity)
                .then(|res| match res {
                    Ok(ctrl_loop) => {
                        //write_to_csv(ctrl_loop.measurements.clone());
                        log::info!("{:?}", ctrl_loop.measurements.generic.clone());
                        Either::A(futures::future::ok(ctrl_loop))
                    }
                    Err((err, ctrl_loop)) => {
                        log::info!("{:?}", ctrl_loop.measurements.generic.clone());
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
