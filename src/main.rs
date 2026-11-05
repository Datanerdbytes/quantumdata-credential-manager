mod cli;
mod config;
mod models;
mod notion;
mod secrets;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("Application error: {error}");
    }
}
