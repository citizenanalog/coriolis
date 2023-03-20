use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub ComPort: Vec<String>,
    pub ModbusAddress: u8,
    pub Regs: Vec<u16>,
    pub cycle_time: u64,
    pub timeout: u64,
}

pub fn read_config() -> Config {
    // Open the configuration file
    let mut file = File::open("config.yml").expect("Failed to open config file");

    // Read the file contents into a string
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Failed to read config file");

    // Deserialize the YAML into a Config struct
    let config: Config = serde_yaml::from_str(&contents).expect("Failed to parse YAML");

    // Extract the Regs field and print it
    let regs: &Vec<u16> = config.Regs.as_ref();
    let com: &Vec<String> = config.ComPort.as_ref();
    let interval = config.cycle_time;
    let timeout = config.timeout;
    println!(
        "Initializing {:?} with regs: {:?}, logging interval {interval} ms, and timeout {timeout} ms.",
        com, regs
    );
    config
}
