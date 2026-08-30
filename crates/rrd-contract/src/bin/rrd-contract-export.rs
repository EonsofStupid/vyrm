use std::io::{self, Write};

fn main() {
    if std::env::args().len() != 1 {
        eprintln!("usage: rrd-contract-export");
        std::process::exit(2);
    }
    let document = match rrd_contract::openapi_document() {
        Ok(document) => document,
        Err(error) => {
            eprintln!("RRD contract export failed: {error}");
            std::process::exit(1);
        }
    };
    let mut encoded = match serde_json::to_vec_pretty(&document) {
        Ok(encoded) => encoded,
        Err(error) => {
            eprintln!("RRD contract encoding failed: {error}");
            std::process::exit(1);
        }
    };
    encoded.push(b'\n');
    if let Err(error) = io::stdout().lock().write_all(&encoded) {
        eprintln!("RRD contract write failed: {error}");
        std::process::exit(1);
    }
}
