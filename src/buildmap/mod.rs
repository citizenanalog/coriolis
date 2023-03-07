use csv::Reader;
use std::collections::HashMap;
use std::fs::File;

pub fn build_hashmap(path: &str) -> HashMap<u16, String> {
    let file: File = File::open(path).unwrap();
    let mut rdr: Reader<File> = Reader::from_reader(file);
    let mut hmap: HashMap<u16, String> = HashMap::new();
    for result in rdr.records() {
        let record = result.unwrap();
        let reg_type = record.get(0);
        //change this to match empty string
        let reg = match reg_type {
            Some(reg) => {
                if "" == reg.to_string() {
                    "U8".to_string() //default type
                } else {
                    reg.to_string()
                }
            }
            None => {
                println!("reg_type: {:?}", reg_type);
                "U8".to_string() //default type
            }
        };

        let addr = record.get(1);
        match addr {
            Some(a) => {
                let address = a.to_string().parse::<u16>().ok().unwrap();
                hmap.entry(address).or_insert(reg);
            }
            None => continue, //ignore cells w/no address
        };
    }
    return hmap;
}
